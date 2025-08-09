mod common;

use assert_cmd::Command;
use common::TestEnv;
use serde_json::json;
use std::fs;
use std::process::Command as StdCommand;
use wiremock::{
    matchers::{method, path_regex},
    Mock, ResponseTemplate,
};

#[tokio::test]
async fn test_run_command_e2e() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create config file for the `run` command
    let config_content = r#"
objective: "Implement a new feature"
files:
  - "src/main.rs"
validate_cmd: "true"
"#;
    env.create_file("run.yml", config_content);
    env.create_file("src/main.rs", "fn main() {}");

    // 2. Mock Gemini for the 'plan' stage
    let plan_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "1. Do this.\n2. Do that."}]
            },
            "finishReason": "STOP"
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-1.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // 3. Mock `aider` to succeed on the first try for the 'implement' stage
    let mock_aider_path = env.full_path("mock_aider.sh");
    let script_content = format!(
        "#!/bin/bash\necho 'modified' > {}\nexit 0",
        env.full_path("src/main.rs").to_str().unwrap()
    );
    fs::write(&mock_aider_path, script_content).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_aider_path)
        .status()
        .unwrap();

    // 4. Run the `aide-rs run` command
    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("run").arg("run.yml");

    // 5. Assertions
    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    assert!(stderr.contains("Starting run from config file."));
    assert!(stderr.contains("Starting plan strategy."));
    assert!(stderr.contains("Plan saved to"));
    assert!(stderr.contains("Starting implement strategy."));
    assert!(stderr.contains("Aider succeeded on attempt 1/5."));
    assert!(stderr.contains("Committing changes"));

    let last_commit = env.get_last_commit_message();
    assert!(last_commit.contains("Implement: Implement the tasks described in the plan file"));
}

#[tokio::test]
async fn test_plan_command_e2e() {
    let env = TestEnv::new().await;
    env.init_git_repo();
    env.create_file("src/main.rs", "fn main() {}");

    // 1. Mock Gemini for the 'plan' stage
    let plan_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "1. This is the plan."}]
            },
            "finishReason": "STOP"
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-1.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // 2. Mock `aider` to check the prompt it receives
    let mock_aider_path = env.full_path("mock_aider.sh");
    let aider_prompt_file = env.full_path("aider_prompt.txt");
    let script_content = format!(
        r#"#!/bin/bash
        # The prompt is passed via --message. The arguments are:
        # $1: --chat-history-file, $2: <path>, $3: src/main.rs, $4: <plan.md>, $5: --message, $6: <prompt>
        echo "$6" > {}
        exit 0
        "#,
        aider_prompt_file.to_str().unwrap()
    );
    fs::write(&mock_aider_path, script_content).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_aider_path)
        .status()
        .unwrap();

    // 3. Run the `aide-rs plan` command
    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("plan")
        .arg("a test plan objective")
        .arg("src/main.rs");

    // 4. Assertions
    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    assert!(stderr.contains("Starting plan strategy."));
    assert!(stderr.contains("Plan saved to"));
    assert!(stderr.contains("Launching aider to review and refine the plan."));

    // Check that the plan file was created
    let log_dir = env.path().join(".ai/sessions");
    let session_dir = fs::read_dir(log_dir).unwrap().next().unwrap().unwrap().path();
    let plan_file = session_dir.join("plan.md");
    assert!(plan_file.exists());
    let plan_content = fs::read_to_string(plan_file).unwrap();
    assert_eq!(plan_content, "1. This is the plan.");

    // Check that aider was called with the correct refinement prompt
    let aider_prompt = fs::read_to_string(aider_prompt_file).unwrap();
    assert!(aider_prompt.contains("Here is the plan I generated"));
    assert!(aider_prompt.contains("plan.md"));
    assert!(aider_prompt.contains("Please review it and help me refine it."));
}

#[tokio::test]
async fn test_research_command_e2e() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Mock Gemini for the 'research' stage
    let research_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "This is the research result."}]
            },
            "finishReason": "STOP"
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-1.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(research_response))
        .mount(&env.mock_server)
        .await;

    // 2. Mock `aider` to check the prompt it receives
    let mock_aider_path = env.full_path("mock_aider.sh");
    let aider_prompt_file = env.full_path("aider_prompt.txt");
    let script_content = format!(
        r#"#!/bin/bash
        # The prompt is passed via --message. The arguments are:
        # $1: --chat-history-file, $2: <path>, $3: <research.md>, $4: --message, $5: <prompt>
        echo "$5" > {}
        exit 0
        "#,
        aider_prompt_file.to_str().unwrap()
    );
    fs::write(&mock_aider_path, script_content).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_aider_path)
        .status()
        .unwrap();

    // 3. Run the `aide-rs research` command
    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("research")
        .arg("a test research objective")
        .arg("src/main.rs");

    // 4. Assertions
    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    assert!(stderr.contains("Starting research strategy."));
    assert!(stderr.contains("Research summary saved to"));
    assert!(stderr.contains("Launching aider to review and refine the research document."));

    // Check that the research file was created
    let log_dir = env.path().join(".ai/sessions");
    let session_dir = fs::read_dir(log_dir).unwrap().next().unwrap().unwrap().path();
    let research_file = session_dir.join("research.md");
    assert!(research_file.exists());
    let research_content = fs::read_to_string(research_file).unwrap();
    assert_eq!(research_content, "This is the research result.");

    // Check that aider was called with the correct refinement prompt
    let aider_prompt = fs::read_to_string(aider_prompt_file).unwrap();
    assert!(aider_prompt.contains("Here is the research document I generated. Please review it."));
}
