use aide_rs::vcs::add_and_commit;
use assert_cmd::Command;
use git2::Repository;
use serde_json::json;
use std::fs;
use wiremock::matchers::{body_string_contains, method, path_regex};
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
                "role": "model",
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
        .and(path_regex(r"/gemini-2.5-flash:generateContent.*"))
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
                "role": "model",
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
        .and(path_regex(r"/gemini-2.5-pro:generateContent.*"))
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
                "role": "model",
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
        .and(path_regex(r"/gemini-2.5-pro:generateContent.*"))
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

#[tokio::test]
async fn test_plan_workflow_with_google_search() {
    let env = TestEnv::new().await;

    // 1. Create a sample prompt file with the search flag enabled
    let prompt_content = r#"
objective = "Create a web server"
use_google_search_for_deps = true
file_scoping.include = ["src/**/*.rs"]
coding_conventions = "Use snake_case"
validation_commands = []
"#;
    env.create_file("search_prompt.toml", prompt_content);

    // 2. Mock the Gemini API response for the dependency search
    let search_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{ "text": "Use `axum` and `tokio`." }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-2.5-flash:generateContent.*"))
        .and(body_string_contains("please use Google Search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(search_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 3. Mock the Gemini API response for the planning step
    let plan_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "create_implementation_plan",
                        "args": { "tasks": [{ "description": "Add axum to Cargo.toml", "file_scoping": { "include": ["Cargo.toml"] }, "validation_steps": [] }] }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-2.5-flash:generateContent.*"))
        .and(body_string_contains("Suggested Libraries (from Google Search)"))
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 4. Run the `aide plan` command
    get_aide_cmd()
        .current_dir(env.path())
        .arg("plan")
        .arg("--prompt")
        .arg("search_prompt.toml")
        .assert()
        .success();

    // 5. Assert that the plan file was created correctly
    let plan_path = env.full_path(".ai/implementation_plan.json");
    assert!(plan_path.exists());
    let plan_content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan_path).unwrap()).unwrap();

    assert_eq!(
        plan_content["tasks"][0]["description"],
        "Add axum to Cargo.toml"
    );
}

#[tokio::test]
async fn test_impl_workflow_with_error_summarization() {
    let env = TestEnv::new().await;

    // 1. Create a project with a file that will cause a validation error
    let long_error_str = "a very long error message...".repeat(100);
    let initial_content = format!("fn main() {{ compile_error!(\"{}\"); }}", long_error_str);
    env.create_file("src/main.rs", &initial_content);
    env.create_file(
        "Cargo.toml",
        "[package]\nname = \"test-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );

    let plan_content = json!({
        "original_prompt": {
            "objective": "Fix compile error",
            "file_scoping": { "include": ["src/main.rs"] },
            "coding_conventions": "",
            "validation_commands": [{ "command": "cargo check", "expected_exit_code": 0 }]
        },
        "tasks": [{
            "description": "Fix the compile error in main.rs",
            "file_scoping": { "include": ["src/main.rs"] },
            "validation_steps": [{ "command": "cargo check", "expected_exit_code": 0 }],
            "status": "Pending",
            "attempts": 0,
            "result": null
        }]
    });
    env.create_file(
        ".ai/implementation_plan.json",
        &plan_content.to_string(),
    );

    // 2. Mock the summarization API response
    let summary_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{ "text": "Summarized: compile_error!" }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-2.5-flash:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(summary_response))
        .mount(&env.mock_server)
        .await;

    // 3. Mock the implementation API response (with the fix)
    let impl_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/main.rs",
                            "new_content": "fn main() { println!(\"Fixed!\"); }"
                        }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response))
        .mount(&env.mock_server)
        .await;

    // 4. Run `aide impl`
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(".ai/implementation_plan.json")
        .assert()
        .success();

    // 5. Assert file was fixed
    let new_content = fs::read_to_string(env.full_path("src/main.rs")).unwrap();
    assert!(new_content.contains("Fixed!"));
}
