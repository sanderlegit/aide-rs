use assert_cmd::Command;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use wiremock::matchers::{body_string_contains, method, path_regex};
use wiremock::{Mock, ResponseTemplate};

mod common;
use common::TestEnv;

fn get_aide_cmd() -> Command {
    let mut cmd = Command::cargo_bin("aide-rs").unwrap();
    let program_path = PathBuf::from(cmd.get_program());
    if let Some(bin_dir) = program_path.parent() {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = std::env::split_paths(&path_var).collect::<Vec<_>>();
        paths.insert(0, bin_dir.to_path_buf());
        let new_path = std::env::join_paths(paths).unwrap();
        cmd.env("PATH", new_path);
    }
    cmd
}

#[tokio::test]
async fn test_e2e_plan_flow() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create the flow and context files
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let flow_content = fs::read_to_string("flows/plan.yml").unwrap();
    env.create_file("flows/plan.yml", &flow_content);

    fs::create_dir_all(env.full_path("ctx")).unwrap();
    let base_scope_content = fs::read_to_string("ctx/base.yaml").unwrap();
    env.create_file("ctx/base.yaml", &base_scope_content);

    // 2. Create the prompt file
    env.create_file(
        "my_prompt.toml",
        r#"
        objective = "Create a hello world app"
        [file_scoping]
        include = ["src/**/*.rs"]
        "#,
    );

    // 3. Create a dummy file for context
    env.create_file("src/main.rs", "fn main() {}");

    // 4. Mock the Gemini API call
    let response_body = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "create_task_list",
                        "args": {
                            "tasks": [
                                { "id": "create-file", "description": "Create a new main.rs file." },
                                { "id": "add-code", "description": "Add hello world code to main.rs." }
                            ]
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-flash-latest:generateContent"))
        .and(body_string_contains("Create a hello world app"))
        .and(body_string_contains("fn main() {}"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&env.mock_server)
        .await;

    // 5. Run the command
    let mut cmd = get_aide_cmd();
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("plan")
        .arg("--prompt")
        .arg("my_prompt.toml");

    // 6. Assert the command succeeds and logs correctly
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Flow 'plan' finished."));
    assert!(stderr.contains("Executing block: 'generate_tasks'..."));
    assert!(stderr.contains("TOOL CALL: create_task_list"));
}

#[tokio::test]
async fn test_e2e_code_flow_single_task() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create flow and context files
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let code_flow_content = fs::read_to_string("flows/code.yml").unwrap();
    env.create_file("flows/code.yml", &code_flow_content);

    fs::create_dir_all(env.full_path("ctx")).unwrap();
    let base_scope_content = fs::read_to_string("ctx/base.yaml").unwrap();
    env.create_file("ctx/base.yaml", &base_scope_content);

    // 2. Create prompt and initial project files
    env.create_file(
        "my_code_prompt.toml",
        r#"
        objective = "Add a hello world function to lib.rs"
        [file_scoping]
        include = ["src/lib.rs"]
        "#,
    );
    env.create_file("src/lib.rs", "// Initial content\n");

    // 3. Mock API calls
    // Mock 1: The 'generate_tasks' block
    let plan_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "create_task_list",
                        "args": {
                            "tasks": [
                                { "id": "add-hello", "description": "Add hello_world function to src/lib.rs" }
                            ]
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(body_string_contains("You are an expert software architect."))
        .and(body_string_contains("Add a hello world function")) // From objective
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2: The 'implement_tasks' block
    let impl_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/lib.rs",
                            "content": "pub fn hello_world() { println!(\"Hello, world!\"); }"
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(body_string_contains("You are an expert pair programmer."))
        .and(body_string_contains("Current Task")) // From implement_tasks prompt
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response))
        .mount(&env.mock_server)
        .await;

    // 4. Run the command
    let mut cmd = get_aide_cmd();
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("code")
        .arg("--prompt")
        .arg("my_code_prompt.toml");

    // 5. Assert success and file modification
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Flow 'code' finished."));

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("pub fn hello_world"));
}

#[tokio::test]
async fn test_e2e_code_flow_with_retry() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create flow and context files
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let code_flow_content = fs::read_to_string("flows/code.yml").unwrap();
    env.create_file("flows/code.yml", &code_flow_content);

    fs::create_dir_all(env.full_path("ctx")).unwrap();
    let base_scope_content = fs::read_to_string("ctx/base.yaml").unwrap();
    env.create_file("ctx/base.yaml", &base_scope_content);

    // 2. Create prompt and initial project files
    env.create_file(
        "my_retry_prompt.toml",
        r#"
        objective = "Add a public function `go()` to lib.rs"
        [file_scoping]
        include = ["src/lib.rs", "Cargo.toml"]
        "#,
    );
    // Need Cargo.toml for cargo check to work
    env.create_file(
        "Cargo.toml",
        r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#,
    );
    env.create_file("src/lib.rs", "// Initial content\n");

    // 3. Mock API calls
    // Mock 1: The 'generate_tasks' block
    let plan_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "create_task_list",
                        "args": {
                            "tasks": [
                                { "id": "add-go-fn", "description": "Add a public function `go()` to lib.rs" }
                            ]
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(body_string_contains("You are an expert software architect."))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2: The 'implement_tasks' block - FIRST attempt (with syntax error)
    let impl_response_fail = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/lib.rs",
                            "content": "pub fn go() { println!(\"go!\") }" // Missing semicolon
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(body_string_contains("You are an expert pair programmer."))
        .and(body_string_contains("Current Task"))
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response_fail))
        .mount(&env.mock_server)
        .await;

    // Mock 3: The 'implement_tasks' block - SECOND attempt (with fix)
    let impl_response_success = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/lib.rs",
                            "content": "pub fn go() { println!(\"go!\"); }" // Corrected
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(body_string_contains("The last attempt failed validation.")) // From on_failure_prompt
        .and(body_string_contains("expected `;`")) // From cargo check stderr
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response_success))
        .mount(&env.mock_server)
        .await;

    // 4. Run the command
    let mut cmd = get_aide_cmd();
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("code")
        .arg("--prompt")
        .arg("my_retry_prompt.toml");

    // 5. Assert success and file modification
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Flow 'code' finished."));
    assert!(stderr.contains("Verification failed for block 'implement_tasks'. Retrying"));

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("println!(\"go!\");"));
}
