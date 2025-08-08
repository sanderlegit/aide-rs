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

    // 1. Create the flow file
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let flow_content = fs::read_to_string("flows/plan.yml").unwrap();
    env.create_file("flows/plan.yml", &flow_content);

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
        .and(path_regex(
            r"/v1beta/models/gemini-1.5-flash-latest:generateContent",
        ))
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
