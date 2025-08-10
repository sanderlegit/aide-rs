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
    println!(
        "Test temp dir for run_command_e2e: {}",
        env.path().display()
    );
    env.init_git_repo();

    // 1. Create config file for the `run` command
    let config_content = r#"
steps:
  - type: plan
    objective: "Implement a new feature"
    context: "all"
  - type: implement
    objective: "Implement the plan"
    context: "all"
    validateCmd: "true"
"#;
    env.create_file("run.yml", config_content);
    env.create_file("src/main.rs", "fn main() {}");
    env.create_file(".ai/filter=all", "#include\n*.rs");

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
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // 3. Mock `aider` to succeed, then report no changes to complete the loop.
    let mock_aider_path = env.full_path("mock_aider.sh");
    let counter_file = env.full_path("run_count.txt");
    fs::write(&counter_file, "0").unwrap();

    let script_content = format!(
        r#"#!/bin/bash
        RUN_COUNT=$(cat {counter})
        if [ "$RUN_COUNT" -eq "0" ]; then
            echo "first run, making changes"
            echo "modified" > {main_rs}
            echo "1" > {counter}
            exit 0
        else
            echo "second run, no changes needed"
            echo "No changes were applied."
            exit 0
        fi
        "#,
        counter = counter_file.to_str().unwrap(),
        main_rs = env.full_path("src/main.rs").to_str().unwrap()
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
    assert!(stderr.contains("Running plan step."));
    assert!(stderr.contains("Plan saved to"));
    assert!(stderr.contains("Running implement step."));
    assert!(stderr.contains("Validation passed on attempt 1/5. Continuing with plan."));
    assert!(stderr.contains(
        "Aider reported no changes on attempt 2/5. Assuming completion."
    ));
    assert!(stderr.contains("Implement strategy completed successfully."));
}

#[tokio::test]
async fn test_plan_command_e2e() {
    let env = TestEnv::new().await;
    println!(
        "Test temp dir for plan_command_e2e: {}",
        env.path().display()
    );
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
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // 2. Mock `aider` to check the prompt it receives
    let mock_aider_path = env.full_path("mock_aider.sh");
    let aider_prompt_file = env.full_path("aider_prompt.txt");
    let script_content = format!(
        r#"#!/bin/bash
                for i in $(seq 1 $#); do
                    if [ "${{!i}}" == "--message" ]; then
                        j=$((i+1))
                        echo "${{!j}}" > {}
                        break
                    fi
                done
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
    println!(
        "Test temp dir for research_command_e2e: {}",
        env.path().display()
    );
    env.init_git_repo();
    env.create_file("src/main.rs", "fn main() {}");

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
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(research_response))
        .mount(&env.mock_server)
        .await;

    // 2. Mock `aider` to check the prompt it receives
    let mock_aider_path = env.full_path("mock_aider.sh");
    let aider_prompt_file = env.full_path("aider_prompt.txt");
    let script_content = format!(
        r#"#!/bin/bash
                for i in $(seq 1 $#); do
                    if [ "${{!i}}" == "--message" ]; then
                        j=$((i+1))
                        echo "${{!j}}" > {}
                        break
                    fi
                done
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

#[tokio::test]
async fn test_run_command_e2e_with_debug_loop() {
    let env = TestEnv::new().await;
    println!(
        "Test temp dir for run_command_e2e_with_debug_loop: {}",
        env.path().display()
    );
    env.init_git_repo();

    // 1. Create a test crate for doc_retriever to use
    let lib_content = r#"
        //! A test crate
        /// A test function that will be "retrieved" by the debug loop.
        pub fn the_fix() {}
    "#;
    env.create_test_crate("test_crate", lib_content);

    // 2. Create config file for the `run` command
    let config_content = r#"
steps:
  - type: plan
    objective: "Implement a feature that will initially fail"
    context: "all"
  - type: implement
    objective: "Implement the plan"
    context: "all"
    validateCmd: "true" # The mock aider script controls success/failure
"#;
    env.create_file("run.yml", config_content);
    env.create_file("src/main.rs", "fn main() {}");
    env.create_file(
        ".ai/filter=all",
        "#include\n*.rs\n*.toml\nplans/*.md\nresearch/*.md",
    );

    // 3. Mock Gemini for the 'plan' stage
    let plan_response = json!({
        "candidates": [{
            "content": { "role": "model", "parts": [{"text": "1. This is the plan."}] },
            "finishReason": "STOP"
        }]
    });
    // 4. Mock Gemini for the 'implement' debug stage
    let implement_debug_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"functionCall": { "name": "doc_retriever", "args": { "crate_name": "test_crate", "path": "test_crate::the_fix" } } }]
            },
            "finishReason": "TOOL_USE"
        }]
    });

    // The first POST is for the plan, the second is for the debug step.
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .up_to_n_times(1)
        .mount(&env.mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(implement_debug_response))
        .up_to_n_times(1)
        .mount(&env.mock_server)
        .await;

    // 5. Mock `aider` and the validation command.
    // Aider will succeed, but capture the prompt on the second real run.
    // The validation command will fail once, then succeed.
    let aider_counter_file = env.full_path("aider_run_count.txt");
    fs::write(&aider_counter_file, "0").unwrap();
    let docs_prompt_file = env.full_path("docs_prompt.txt");
    let mock_aider_path = env.full_path("mock_aider.sh");
    let aider_script = format!(
        r#"#!/bin/bash
        RUN_COUNT=$(cat {counter})
        if [ "$RUN_COUNT" -eq "0" ]; then
            echo "first aider run"
            echo "1" > {counter}
            exit 0
        elif [ "$RUN_COUNT" -eq "1" ]; then
            echo "second aider run (with docs)"
            for i in $(seq 1 $#); do
                if [ "${{!i}}" == "--message" ]; then
                    j=$((i+1))
                    echo "${{!j}}" > {docs_prompt}
                    break
                fi
            done
            echo "2" > {counter}
            exit 0
        else
            echo "third aider run (no changes)"
            echo "No changes were applied."
            exit 0
        fi
        "#,
        counter = aider_counter_file.to_str().unwrap(),
        docs_prompt = docs_prompt_file.to_str().unwrap(),
    );
    fs::write(&mock_aider_path, aider_script).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_aider_path)
        .status()
        .unwrap();

    // The validation command fails on its first run.
    let validate_counter_file = env.full_path("validate_run_count.txt");
    fs::write(&validate_counter_file, "0").unwrap();
    let mock_validate_path = env.full_path("mock_validate.sh");
    let validate_script = format!(
        r#"#!/bin/bash
        RUN_COUNT=$(cat {counter})
        if [ "$RUN_COUNT" -eq "0" ]; then
            echo "validation failing"
            echo "1" > {counter}
            exit 1
        else
            echo "validation succeeding"
            exit 0
        fi
        "#,
        counter = validate_counter_file.to_str().unwrap()
    );
    fs::write(&mock_validate_path, validate_script).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_validate_path)
        .status()
        .unwrap();

    // Replace the validate_cmd in the config file with our mock script
    let config_content = fs::read_to_string(env.full_path("run.yml")).unwrap();
    let new_config_content =
        config_content.replace("true", &format!("\"{}\"", mock_validate_path.to_str().unwrap()));
    env.create_file("run.yml", &new_config_content);

    // 6. Run the `aide-rs run` command
    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("run").arg("run.yml");

    // 7. Assertions
    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    // Check that the full flow happened
    assert!(stderr.contains("Starting run from config file."));
    assert!(stderr.contains("Running plan step."));
    assert!(stderr.contains("Plan saved to"));
    assert!(stderr.contains("Running implement step."));
    assert!(stderr.contains("Validation failed on attempt 1/5. Analyzing failure..."));
    assert!(stderr.contains("Gemini requested a tool call for debugging"));
    assert!(stderr.contains("Retrieved documentation"));
    assert!(stderr.contains("Validation passed on attempt 2/5. Continuing with plan."));
    assert!(stderr.contains(
        "Aider reported no changes on attempt 3/5. Assuming completion."
    ));
    assert!(stderr.contains("Implement strategy completed successfully."));

    // Check that the docs were passed to aider on the second run
    let final_prompt = fs::read_to_string(docs_prompt_file).unwrap();
    assert!(final_prompt.contains("pub fn the_fix()"));
    assert!(final_prompt.contains("Please use this information to fix the code."));
}
