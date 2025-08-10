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
async fn test_implement_auto_success_on_first_try() {
    let env = TestEnv::new().await;
    println!(
        "Test temp dir for implement_auto_success_on_first_try: {}",
        env.path().display()
    );
    env.init_git_repo();
    env.create_file("src/main.rs", "fn main() {}");

    // Create a mock `aider` script that simulates success and modifies a file.
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

    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("implement")
        .arg("a test objective")
        .arg("src/main.rs")
        .arg("--auto")
        .arg("--validate-cmd")
        .arg("true");

    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    assert!(stderr.contains("Starting implement strategy."));
    assert!(stderr.contains("Running aider in auto mode."));
    assert!(stderr.contains("Aider succeeded on attempt 1/5."));
    assert!(stderr.contains("Committing changes with message: Implement: a test objective"));

    let last_commit = env.get_last_commit_message();
    assert_eq!(last_commit, "Implement: a test objective");
}

#[tokio::test]
async fn test_implement_auto_failure_and_retry() {
    let env = TestEnv::new().await;
    println!(
        "Test temp dir for implement_auto_failure_and_retry: {}",
        env.path().display()
    );

    // Mock the Gemini API for the debug step.
    // It should suggest using the doc_retriever tool.
    let mock_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "doc_retriever",
                        "args": { "crate_name": "some_crate", "path": "some_crate::some_module" }
                    }
                }]
            },
            "finishReason": "TOOL_USE"
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    env.init_git_repo();
    env.create_file("src/main.rs", "fn main() {}");

    // This mock script will fail on the first run and succeed on the second.
    // We'll use a counter file to track runs.
    let counter_file = env.full_path("run_count.txt");
    fs::write(&counter_file, "0").unwrap();

    let mock_aider_path = env.full_path("mock_aider.sh");
    let script_content = format!(
        r#"#!/bin/bash
        RUN_COUNT=$(cat {counter})
        if [ "$RUN_COUNT" -eq "0" ]; then
            echo "first run, failing"
            echo "0" > {main_rs} # make a change
            echo "1" > {counter}
            exit 1
        else
            echo "second run, succeeding"
            echo "1" > {main_rs} # make another change
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

    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("implement")
        .arg("a retry objective")
        .arg("src/main.rs")
        .arg("--auto")
        .arg("--validate-cmd")
        .arg("true"); // This doesn't matter since aider itself fails/succeeds

    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    // Check logs for both attempts
    assert!(stderr.contains("Aider failed on attempt 1/5. Analyzing failure..."));
    assert!(stderr.contains("Aider succeeded on attempt 2/5."));
    assert!(stderr.contains("Committing changes with message: Implement: a retry objective"));

    let last_commit = env.get_last_commit_message();
    assert_eq!(last_commit, "Implement: a retry objective");
}

#[tokio::test]
async fn test_implement_auto_failure_and_debug_with_docs() {
    let env = TestEnv::new().await;
    println!(
        "Test temp dir for implement_auto_failure_and_debug_with_docs: {}",
        env.path().display()
    );

    // 1. Setup a test crate that will be used by doc_retriever
    let lib_content = r#"
        //! A test crate
        /// A test function
        pub fn old_function() {}
    "#;
    env.create_test_crate("test_crate", lib_content);

    // 2. Mock Gemini to request docs for the test crate's function
    let mock_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "doc_retriever",
                        "args": { "crate_name": "test_crate", "path": "test_crate::old_function" }
                    }
                }]
            },
            "finishReason": "TOOL_USE"
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    env.init_git_repo();
    // The file to be "edited" by aider.
    env.create_file("src/main.rs", "fn main() {}");

    // 3. Mock aider to fail the first time, and succeed the second time.
    // The second run's prompt will be checked to see if it contains the docs.
    let counter_file = env.full_path("run_count.txt");
    fs::write(&counter_file, "0").unwrap();
    let docs_prompt_file = env.full_path("docs_prompt.txt");

    let mock_aider_path = env.full_path("mock_aider.sh");
    let script_content = format!(
        r#"#!/bin/bash
        RUN_COUNT=$(cat {counter})
        if [ "$RUN_COUNT" -eq "0" ]; then
            echo "first run, failing"
            echo "1" > {counter}
            exit 1
        else
            # On second run, capture the prompt and succeed
            echo "second run, succeeding"
            # The prompt is passed via --message. The arguments are:
            # $1: --chat-history-file, $2: <path>, $3: src/main.rs, $4: --message, $5: <prompt>
            echo "$5" > {docs_prompt}
            exit 0
        fi
        "#,
        counter = counter_file.to_str().unwrap(),
        docs_prompt = docs_prompt_file.to_str().unwrap(),
    );
    fs::write(&mock_aider_path, script_content).unwrap();
    StdCommand::new("chmod")
        .arg("+x")
        .arg(&mock_aider_path)
        .status()
        .unwrap();

    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    cmd.current_dir(env.path());
    env.apply_env(&mut cmd);
    cmd.env("AIDER_COMMAND", mock_aider_path.to_str().unwrap());

    cmd.arg("implement")
        .arg("an objective that requires docs")
        .arg("src/main.rs")
        .arg("--auto")
        .arg("--validate-cmd")
        .arg("true");

    let output = cmd.assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    // 4. Assertions
    assert!(stderr.contains("Aider failed on attempt 1/5. Analyzing failure..."));
    assert!(stderr.contains("Gemini requested a tool call for debugging"));
    assert!(stderr.contains("Retrieved documentation"));
    assert!(stderr.contains("Aider succeeded on attempt 2/5."));

    let final_prompt = fs::read_to_string(docs_prompt_file).unwrap();
    assert!(final_prompt.contains("pub fn old_function()"));
    assert!(final_prompt.contains("Please use this information to fix the code."));
}
