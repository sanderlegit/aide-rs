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

    // 2. Mock API responses for the two-step planning process
    let descriptions_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "create_task_descriptions",
                        "args": { "tasks": ["First task: Refactor the main function."] }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-flash:generateContent.*"))
        .and(body_string_contains("create_task_descriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(descriptions_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    let details_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "create_task_details",
                        "args": {
                            "validation_steps": [{ "command": "cargo check", "expected_exit_code": 0 }]
                        }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-flash:generateContent.*"))
        .and(body_string_contains("create_task_details"))
        .respond_with(ResponseTemplate::new(200).set_body_json(details_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide plan` command
    let output = get_aide_cmd()
        .current_dir(env.path())
        .arg("plan")
        .arg("--prompt")
        .arg("my_feature.toml")
        .assert()
        .success()
        .get_output()
        .clone();

    // 4. Assert that the plan file was created correctly
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("\n--- E2E TEST: test_plan_workflow ---\n--- STDOUT ---\n{}\n--- STDERR ---\n{}\n---\n", stdout, stderr);
    let plan_path_str = stderr
        .lines()
        .find(|line| line.contains("Implementation plan saved to"))
        .and_then(|line| line.split('"').nth(1).map(|s| s.to_string()))
        .expect("Could not find plan path in command output");

    let plan_path = env.full_path(&plan_path_str);
    assert!(plan_path.exists(), "Plan file should exist");

    let plan_content: toml::Value =
        toml::from_str(&fs::read_to_string(plan_path).unwrap()).unwrap();

    assert_eq!(
        plan_content["tasks"][0]["description"].as_str(),
        Some("First task: Refactor the main function.")
    );
    assert_eq!(
        plan_content["tasks"][0]["status"].as_str(),
        Some("Pending")
    );
    assert_eq!(
        plan_content["tasks"][0]["validation_steps"][0]["command"].as_str(),
        Some("cargo check")
    );
}

#[tokio::test]
async fn test_impl_workflow() {
    let env = TestEnv::new().await;

    // 1. Create a sample project and plan file
    env.create_file("src/main.rs", "fn main() {\n    println!(\"Hello, old world!\");\n}\n");
    let plan_content = r#"
[original_prompt]
objective = "Update greeting"
coding_conventions = ""

[original_prompt.file_scoping]
include = ["src/main.rs"]
exclude = []

[[original_prompt.validation_commands]]
command = "cargo check"
expected_exit_code = 0

[[tasks]]
description = "Update the greeting message in main.rs"
status = "Pending"
attempts = 0
validation_steps = []
    "#;
    let plan_path = ".ai/test_plan.toml";
    env.create_file(plan_path, plan_content);

    // 2. Mock the implementation API response

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
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide impl` command
    let plan_path = ".ai/test_plan.toml";
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(plan_path)
        .assert()
        .success();

    // 4. Assert that the file was modified
    let new_content = fs::read_to_string(env.full_path("src/main.rs")).unwrap();
    assert!(new_content.contains("Hello, new world!"));

    // 5. Assert that the plan was updated
    let plan_content: toml::Value =
        toml::from_str(&fs::read_to_string(env.full_path(plan_path)).unwrap()).unwrap();
    assert_eq!(
        plan_content["tasks"][0]["status"].as_str(),
        Some("Success")
    );
    assert_eq!(
        plan_content["tasks"][0]["result"]["agent_tips"].as_str(),
        Some("Updated the greeting.")
    );
    assert_eq!(
        plan_content["tasks"][0]["result"]["modified_files"][0].as_str(),
        Some("src/main.rs")
    );
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

    let plan_content = r#"
[original_prompt]
objective = "Update greeting"
coding_conventions = ""
validation_commands = []

[original_prompt.file_scoping]
include = ["src/main.rs"]
exclude = []

[[tasks]]
description = "Add a print statement"
status = "Pending"
attempts = 0
validation_steps = []
    "#;
    let plan_path = ".ai/test_plan.toml";
    env.create_file(plan_path, plan_content);

    // 2. Mock the implementation API response

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
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
        .mount(&env.mock_server)
        .await;

    // 3. Run the `aide impl` command with --auto-commit
    let plan_path = ".ai/test_plan.toml";
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(plan_path)
        .arg("--auto-commit")
        .assert()
        .success();

    // 4. Assert that a new commit was created
    let repo = Repository::open(env.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let commit_message = head.message().unwrap();
    assert_eq!(commit_message, "AI: Add a print statement");
}


#[tokio::test]
async fn test_plan_lancedb_prompt_sends_correct_schema() {
    let env = TestEnv::new().await;

    // 1. Copy the prompt file
    let prompt_content = fs::read_to_string("prompts/lancedb_example.toml").unwrap();
    env.create_file("lancedb_example.toml", &prompt_content);

    // 2. Mock the Gemini API response for planning (Step 1: descriptions)
    let descriptions_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"functionCall": {"name": "create_task_descriptions", "args": {"tasks": ["A sample task"]}}}]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-flash:generateContent.*"))
        .and(body_string_contains("create_task_descriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(descriptions_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 3. Mock the Gemini API response for planning (Step 2: details)
    let details_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"functionCall": {"name": "create_task_details", "args": {
                    "validation_steps": []
                }}}]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-flash:generateContent.*"))
        // Key part of the test: assert that the "items" field for the array is present in the request body.
        // This is for the `validation_steps` parameter in the `create_task_details` tool.
        .and(body_string_contains("\"items\":{\"properties\""))
        .and(body_string_contains("create_task_details"))
        .respond_with(ResponseTemplate::new(200).set_body_json(details_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 4. Run the `aide plan` command
    get_aide_cmd()
        .current_dir(env.path())
        .arg("plan")
        .arg("--prompt")
        .arg("lancedb_example.toml")
        .assert()
        .success();
}

#[tokio::test]
async fn test_impl_workflow_with_doc_retriever() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create a project with a file that will cause a validation error
    // This simulates an error where a type is not an iterator.
    let initial_content = r#"
use anyhow::Result;
use std::time::Duration;

fn main() -> Result<()> {
    let d = Duration::new(1, 0);
    for _item in d {
        // this will fail to compile
    }
    Ok(())
}
"#;
    env.create_file("src/main.rs", initial_content);
    env.create_file(
        "Cargo.toml",
        r#"[package]
name = "test-proj"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
"#,
    );

    let plan_content = r#"
[original_prompt]
objective = "Fix compile error"
coding_conventions = ""

[original_prompt.file_scoping]
include = ["src/main.rs", "Cargo.toml"]
exclude = []

[[original_prompt.validation_commands]]
command = "cargo check"
expected_exit_code = 0

[[tasks]]
description = "Fix the compile error in main.rs"
status = "Pending"
attempts = 0
[[tasks.validation_steps]]
command = "cargo check"
expected_exit_code = 0
    "#;
    let plan_path = ".ai/test_plan.toml";
    env.create_file(plan_path, plan_content);

    // 2. Mock the initial LLM call which asks for documentation
    let doc_request_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "doc_retriever",
                        "args": {
                            "subcommand": "type",
                            "crate_name": "std",
                            "path": "std::time::Duration"
                        }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .and(body_string_contains("is not an iterator")) // First prompt contains the error
        .respond_with(ResponseTemplate::new(200).set_body_json(doc_request_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 3. Mock the second LLM call which provides the fix after getting docs
    let fix_response = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/main.rs",
                            "new_content": "use anyhow::Result;\nuse std::time::Duration;\n\nfn main() -> Result<()> {\n    let d = Duration::new(1, 0);\n    // Fixed: Duration is not an iterator.\n    println!(\"Duration is {:?}\", d);\n    Ok(())\n}\n"
                        }
                    }
                }]
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .and(body_string_contains("functionResponse")) // Second prompt contains tool result
        .respond_with(ResponseTemplate::new(200).set_body_json(fix_response))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 4. Run `aide impl` with error enrichment
    let plan_path = ".ai/test_plan.toml";
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(plan_path)
        .arg("--enrich-errors")
        .assert()
        .success();

    // 5. Assert file was fixed
    let new_content = fs::read_to_string(env.full_path("src/main.rs")).unwrap();
    assert!(new_content.contains("Fixed: Duration is not an iterator."));
}

#[tokio::test]
async fn test_impl_multi_task_workflow() {
    let env = TestEnv::new().await;
    env.init_git_repo();
    env.create_file(
        "Cargo.toml",
        "[package]\nname = \"test-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    add_and_commit(
        &env.path(),
        &[env.full_path("Cargo.toml")],
        "Initial commit",
    )
    .unwrap();

    // 1. Create a multi-task plan
    let plan_content = r#"
[original_prompt]
objective = "Create a hello world app using anyhow"
coding_conventions = ""

[original_prompt.file_scoping]
include = ["src/**/*.rs", "Cargo.toml"]
exclude = []

[[original_prompt.validation_commands]]
command = "cargo check"
expected_exit_code = 0

[[tasks]]
description = "Add anyhow dependency to Cargo.toml"
status = "Pending"
attempts = 0
validation_steps = []

[[tasks]]
description = "Create main.rs to print hello world"
status = "Pending"
attempts = 0
[[tasks.validation_steps]]
command = "cargo check"
expected_exit_code = 0
    "#;
    let plan_path = ".ai/test_plan.toml";
    env.create_file(plan_path, plan_content);

    // 2. Mock API responses

    let mock_response_task1 = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "Cargo.toml",
                            "new_content": "[package]\nname = \"test-proj\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nanyhow = \"1.0\""
                        }
                    }
                }]
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .and(body_string_contains(
            "**Current Task:**\\nAdd anyhow dependency to Cargo.toml",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response_task1))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 3. Mock API response for Task 2 (create main.rs)
    let mock_response_task2 = json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "create_file",
                        "args": {
                            "path": "src/main.rs",
                            "content": "fn main() -> anyhow::Result<()> {\n    println!(\"Hello, world!\");\n    Ok(())\n}\n"
                        }
                    }
                }]
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path_regex(r"/models/gemini-2.5-pro:generateContent.*"))
        .and(body_string_contains(
            "**Current Task:**\\nCreate main.rs to print hello world",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_response_task2))
        .expect(1)
        .mount(&env.mock_server)
        .await;

    // 4. Run `aide impl`
    let plan_path = ".ai/test_plan.toml";
    get_aide_cmd()
        .current_dir(env.path())
        .arg("impl")
        .arg("--plan")
        .arg(plan_path)
        .arg("--auto-commit")
        .assert()
        .success();

    // 5. Assert file changes
    let cargo_toml_content = fs::read_to_string(env.full_path("Cargo.toml")).unwrap();
    assert!(cargo_toml_content.contains("anyhow = \"1.0\""));

    let main_rs_content = fs::read_to_string(env.full_path("src/main.rs")).unwrap();
    assert!(main_rs_content.contains("anyhow::Result<()>"));

    // 6. Assert plan status
    let plan_path = ".ai/test_plan.toml";
    let plan_content: toml::Value =
        toml::from_str(&fs::read_to_string(env.full_path(plan_path)).unwrap()).unwrap();
    assert_eq!(
        plan_content["tasks"][0]["status"].as_str(),
        Some("Success")
    );
    assert_eq!(
        plan_content["tasks"][1]["status"].as_str(),
        Some("Success")
    );

    // 7. Assert that two new commits were created
    let repo = Repository::open(env.path()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        head.message().unwrap(),
        "AI: Create main.rs to print hello world"
    );

    let parent = head.parent(0).unwrap();
    assert_eq!(
        parent.message().unwrap(),
        "AI: Add anyhow dependency to Cargo.toml"
    );

    let grandparent = parent.parent(0).unwrap();
    assert_eq!(grandparent.message().unwrap(), "Initial commit");
}
