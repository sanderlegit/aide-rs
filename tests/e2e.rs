use aide_rs::vcs::add_and_commit;
use assert_cmd::Command;
use git2::Repository;
use serde_json::json;
use std::fs;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

mod common;
use common::TestEnv;

fn get_aide_cmd() -> Command {
    Command::cargo_bin("aide-rs").unwrap()
}

#[tokio::test]
async fn test_plan_workflow() {
    let env = TestEnv::new().await;

    // 1. Create a sample prompt file
    let prompt_content = r#"
objective = "Implement a new feature"
file_scoping.include = ["src/**/*.rs"]
coding_conventions = "Use snake_case"
validation_commands = [{ command = "cargo check", expected_exit_code = 0 }]
"#;
    env.create_file("my_feature.toml", prompt_content);

    // 2. Mock the Gemini API response for planning
    let mock_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "create_implementation_plan",
                        "args": {
                            "tasks": [{
                                "description": "First task: Refactor the main function.",
                                "file_scoping": { "include": ["src/main.rs"] },
                                "validation_steps": [{ "command": "cargo check", "expected_exit_code": 0 }]
                            }]
                        }
                    }
                }]
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-1.5-flash:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide plan` command
    get_aide_cmd()
        .current_dir(env.path())
        .arg("plan")
        .arg("--prompt")
        .arg("my_feature.toml")
        .assert()
        .success();

    // 4. Assert that the plan file was created correctly
    let plan_path = env.full_path(".ai/implementation_plan.json");
    assert!(plan_path.exists());
    let plan_content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan_path).unwrap()).unwrap();

    assert_eq!(
        plan_content["tasks"][0]["description"],
        "First task: Refactor the main function."
    );
    assert_eq!(plan_content["tasks"][0]["status"], "Pending");
}

#[tokio::test]
async fn test_impl_workflow() {
    let env = TestEnv::new().await;

    // 1. Create a sample project and plan file
    env.create_file("src/main.rs", "fn main() {\n    println!(\"Hello, old world!\");\n}\n");
    let plan_content = json!({
        "original_prompt": {
            "objective": "Update greeting",
            "file_scoping": { "include": ["src/main.rs"] },
            "coding_conventions": "",
            "validation_commands": [{ "command": "cargo check", "expected_exit_code": 0 }]
        },
        "tasks": [{
            "description": "Update the greeting message in main.rs",
            "file_scoping": { "include": ["src/main.rs"] },
            "validation_steps": [], // No validation for simplicity
            "status": "Pending",
            "attempts": 0,
            "result": null
        }]
    });
    env.create_file(".ai/implementation_plan.json", &plan_content.to_string());

    // 2. Mock the Gemini API response for implementation
    let mock_response = json!({
        "candidates": [{
            "content": {
                "parts": [
                    {
                        "functionCall": {
                            "name": "edit_file",
                            "args": {
                                "path": "src/main.rs",
                                "new_content": "fn main() {\n    println!(\"Hello, new world!\");\n}\n"
                            }
                        }
                    },
                    {
                        "text": "Updated the greeting."
                    }
                ]
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-1.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide impl` command
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(".ai/implementation_plan.json")
        .assert()
        .success();

    // 4. Assert that the file was modified
    let new_content = fs::read_to_string(env.full_path("src/main.rs")).unwrap();
    assert!(new_content.contains("Hello, new world!"));

    // 5. Assert that the plan was updated
    let plan_content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(env.full_path(".ai/implementation_plan.json")).unwrap()).unwrap();
    assert_eq!(plan_content["tasks"][0]["status"], "Success");
    assert_eq!(plan_content["tasks"][0]["result"]["agent_tips"], "Updated the greeting.");
}

#[tokio::test]
async fn test_impl_workflow_with_auto_commit() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create a sample project and plan file
    env.create_file("src/main.rs", "fn main() {}");
    add_and_commit(
        &env.path(),
        &[env.full_path("src/main.rs")],
        "Add main.rs",
    )
    .unwrap();

    let plan_content = json!({
        "original_prompt": {
            "objective": "Update greeting",
            "file_scoping": { "include": ["src/main.rs"] },
            "coding_conventions": "",
            "validation_commands": []
        },
        "tasks": [{
            "description": "Add a print statement",
            "file_scoping": { "include": ["src/main.rs"] },
            "validation_steps": [],
            "status": "Pending",
            "attempts": 0,
            "result": null
        }]
    });
    env.create_file(".ai/implementation_plan.json", &plan_content.to_string());

    // 2. Mock the Gemini API response
    let mock_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/main.rs",
                            "new_content": "fn main() { println!(\"hello\"); }"
                        }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide impl` command with --auto-commit
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(".ai/implementation_plan.json")
        .arg("--auto-commit")
        .assert()
        .success();

    // 4. Assert that a new commit was created
    let repo = Repository::open(env.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let commit_message = head.message().unwrap();
    assert!(commit_message.contains("AI-generated changes for:"));
    assert!(commit_message.contains("- Add a print statement"));
}
