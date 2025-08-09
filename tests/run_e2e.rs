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
