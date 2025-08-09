mod common;

use common::TestEnv;
use assert_cmd::Command;
use std::fs;
use std::process::Command as StdCommand;

#[tokio::test]
async fn test_implement_auto_success_on_first_try() {
    let env = TestEnv::new().await;
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
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();

    assert!(stdout.contains("Starting implement strategy."));
    assert!(stdout.contains("Running aider in auto mode."));
    assert!(stdout.contains("Aider succeeded on attempt 1/5."));
    assert!(stdout.contains("Committing changes with message: Implement: a test objective"));

    let last_commit = env.get_last_commit_message();
    assert_eq!(last_commit, "Implement: a test objective");
}

#[tokio::test]
async fn test_implement_auto_failure_and_retry() {
    let env = TestEnv::new().await;
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
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();

    // Check logs for both attempts
    assert!(stdout.contains("Aider failed on attempt 1/5. Analyzing failure..."));
    assert!(stdout.contains("Aider succeeded on attempt 2/5."));
    assert!(stdout.contains("Committing changes with message: Implement: a retry objective"));

    let last_commit = env.get_last_commit_message();
    assert_eq!(last_commit, "Implement: a retry objective");
}
