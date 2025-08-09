use assert_cmd::Command;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use wiremock::matchers::{body_string_contains, method, path};
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
        "my_prompt.yml",
        r#"
        objective: "Create a hello world app"
        file_scoping:
          include: ["src/**/*.rs"]
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
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 20,
            "totalTokenCount": 30
        }
    });

    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("Create a hello world app"))
        .and(body_string_contains("fn main() {}"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&env.mock_server)
        .await;

    // Mock the response after the tool call to finish the block
    let finish_response = json!({ "candidates": [{"content": { "parts": [{"text": "OK" }], "role": "model" }}] });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response))
        .mount(&env.mock_server)
        .await;

    // 5. Run the command
    let mut cmd = get_aide_cmd();
    env.apply_env(&mut cmd);
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("plan")
        .arg("--prompt")
        .arg("my_prompt.yml");

    // 6. Assert the command succeeds and logs correctly
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Flow 'plan' finished."),
        "Flow did not finish. Stderr:\n---\n{}\n---",
        stderr
    );
    assert!(stderr.contains("Executing block: 'generate_tasks'..."));
    assert!(stderr.contains("TOOL CALL: create_task_list"));

    // 7. Assert that the performance log was created and is correct
    let log_dir = env.full_path(".ai/logs");
    let mut entries = fs::read_dir(log_dir).unwrap();
    let run_dir = entries.next().unwrap().unwrap().path(); // Get the first (and only) run directory
    let perf_log_path = run_dir.join("performance.log.jsonl");
    assert!(perf_log_path.exists(), "Performance log was not created.");

    let perf_log_content = fs::read_to_string(perf_log_path).unwrap();
    assert!(
        !perf_log_content.is_empty(),
        "Performance log is empty."
    );

    // Parse the first line of the performance log
    let first_line = perf_log_content.lines().next().unwrap();
    let log_entry: serde_json::Value = serde_json::from_str(first_line).unwrap();

    // Check the payload
    let payload = &log_entry["payload"];
    assert_eq!(payload["modelName"], "gemini-2.5-pro");
    assert_eq!(payload["promptTokens"], 10);
    assert_eq!(payload["candidatesTokens"], 20);
    assert_eq!(payload["totalTokens"], 30);
    assert_eq!(payload["runningTotalTokens"], 30);
    assert!(payload["timeTakenMs"].is_number());
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
    let ai_scope_content = fs::read_to_string("ctx/ai.yaml").unwrap();
    env.create_file("ctx/ai.yaml", &ai_scope_content);
    fs::create_dir_all(env.full_path("doc")).unwrap();
    let arch_doc_content = fs::read_to_string("doc/refactor_architecture.md").unwrap();
    env.create_file("doc/refactor_architecture.md", &arch_doc_content);

    // 2. Create prompt and initial project files
    env.create_file("Makefile", "test:\n\t@cargo check\n");
    env.create_file(
        "my_code_prompt.yml",
        r#"
        objective: "Add a hello world function to lib.rs"
        file_scoping:
          include: ["src/lib.rs"]
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
    // Mock 1: The 'generate_markdown_plan' block
    let markdown_plan_response = json!({
        "candidates": [{
            "content": {
                "parts": [{ "text": "Plan: Add a function to `src/lib.rs`." }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "This is a creative step to think through the solution",
        )) // Unique to the markdown plan block
        .and(body_string_contains("Add a hello world function to lib.rs")) // From objective
        .respond_with(ResponseTemplate::new(200).set_body_json(markdown_plan_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2: The 'generate_structured_tasks' block
    let structured_task_response = json!({
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("convert the provided markdown plan"))
        .and(body_string_contains(
            "Plan: Add a function to `src/lib.rs`.",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(structured_task_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2.5: The response after sending the tool call result back to the model
    let finish_response = json!({ "candidates": [{"content": { "parts": [{"text": "OK" }], "role": "model" }}] });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("create_task_list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response.clone()))
        .mount(&env.mock_server)
        .await;

    // Mock 3: The 'implement_tasks' block
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "Your goal is to implement the current task.",
        )) // Unique to code.yml implement_tasks
        .and(body_string_contains("**Current Task:**"))
        .and(body_string_contains(
            "Add hello_world function to src/lib.rs",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response))
        .mount(&env.mock_server)
        .await;

    // Mock 3.5: Finish the implementation turn
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("edit_file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response))
        .mount(&env.mock_server)
        .await;

    // 4. Run the command
    let mut cmd = get_aide_cmd();
    env.apply_env(&mut cmd);
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("code")
        .arg("--prompt")
        .arg("my_code_prompt.yml");

    // 5. Assert success and file modification
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Flow 'code' finished."),
        "Flow did not finish. Stderr:\n---\n{}\n---",
        stderr
    );

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("pub fn hello_world"));

    let commit_msg = env.get_last_commit_message();
    assert_eq!(
        commit_msg,
        "aide-rs: auto-commit after task 'add-hello' in flow 'code'"
    );
}

#[tokio::test]
async fn test_e2e_code_flow_with_doc_retriever() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create flow and context files
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let code_flow_content = fs::read_to_string("flows/code.yml").unwrap();
    env.create_file("flows/code.yml", &code_flow_content);

    fs::create_dir_all(env.full_path("ctx")).unwrap();
    let base_scope_content = fs::read_to_string("ctx/base.yaml").unwrap();
    env.create_file("ctx/base.yaml", &base_scope_content);
    let ai_scope_content = fs::read_to_string("ctx/ai.yaml").unwrap();
    env.create_file("ctx/ai.yaml", &ai_scope_content);
    fs::create_dir_all(env.full_path("doc")).unwrap();
    let arch_doc_content = fs::read_to_string("doc/refactor_architecture.md").unwrap();
    env.create_file("doc/refactor_architecture.md", &arch_doc_content);

    // 2. Create prompt and a dummy crate for the tool to inspect
    env.create_file("Makefile", "test:\n\t@cargo check\n");
    env.create_file(
        "my_doc_prompt.yml",
        r#"
        objective: "In `src/lib.rs`, add a call to `test_crate::do_stuff()`"
        file_scoping:
          include:
            - "src/lib.rs"
            - "Cargo.toml"
        "#,
    );
    env.create_file(
        "Cargo.toml",
        r#"[package]
name = "my-project"
version = "0.1.0"
edition = "2021"

[dependencies]
test_crate = { path = "test_crate" }
"#,
    );
    env.create_file("src/lib.rs", "// empty\n");

    // Create the dependency crate that the agent will inspect
    env.create_file(
        "test_crate/Cargo.toml",
        r#"[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#,
    );
    env.create_file(
        "test_crate/src/lib.rs",
        r#"
        /// Does important stuff.
        pub fn do_stuff() -> u32 { 42 }
        "#,
    );

    // 3. Mock API calls
    // Mock 1 & 2: Plan and structure tasks (skipping for brevity, straight to implement)
    let markdown_plan_response = json!({
        "candidates": [{
            "content": { "parts": [{ "text": "Plan: Call the function." }], "role": "model" }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("create a detailed, human-readable implementation plan"))
        .respond_with(ResponseTemplate::new(200).set_body_json(markdown_plan_response))
        .mount(&env.mock_server)
        .await;

    let structured_task_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "create_task_list",
                        "args": { "tasks": [ { "id": "call-it", "description": "Call test_crate::do_stuff" } ] }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("convert the provided markdown plan"))
        .respond_with(ResponseTemplate::new(200).set_body_json(structured_task_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2.5: Finish the structured task generation turn
    let finish_response = json!({ "candidates": [{"content": { "parts": [{"text": "OK" }], "role": "model" }}] });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("create_task_list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response.clone()))
        .mount(&env.mock_server)
        .await;

    // Mock 3: The 'implement_tasks' block, first it calls doc_retriever
    let doc_retriever_call_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "doc_retriever",
                        "args": {
                            "crate_name": "test_crate",
                            "path": "test_crate::do_stuff"
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("Your goal is to implement the current task."))
        .and(body_string_contains("Call test_crate::do_stuff"))
        .respond_with(ResponseTemplate::new(200).set_body_json(doc_retriever_call_response))
        .mount(&env.mock_server)
        .await;

    // Mock 4: The 'implement_tasks' block gets the doc_retriever result and edits the file
    let edit_file_call_response = json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": "edit_file",
                        "args": {
                            "path": "src/lib.rs",
                            "content": "pub fn my_func() { test_crate::do_stuff(); }"
                        }
                    }
                }],
                "role": "model"
            }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse")) // The history now contains the tool result
        .and(body_string_contains("Does important stuff.")) // The doc string from the tool result
        .respond_with(ResponseTemplate::new(200).set_body_json(edit_file_call_response))
        .mount(&env.mock_server)
        .await;

    // Mock 5: Finish the implementation turn after edit_file
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("edit_file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response))
        .mount(&env.mock_server)
        .await;

    // 4. Run the command
    let mut cmd = get_aide_cmd();
    env.apply_env(&mut cmd);
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("code")
        .arg("--prompt")
        .arg("my_doc_prompt.yml");

    // 5. Assert success and file modification
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Flow 'code' finished."),
        "Flow did not finish. Stderr:\n---\n{}\n---",
        stderr
    );
    assert!(stderr.contains("TOOL CALL: doc_retriever"));
    assert!(stderr.contains("TOOL CALL: edit_file"));

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("test_crate::do_stuff()"));

    let commit_msg = env.get_last_commit_message();
    assert_eq!(
        commit_msg,
        "aide-rs: auto-commit after task 'call-it' in flow 'code'"
    );
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
    let ai_scope_content = fs::read_to_string("ctx/ai.yaml").unwrap();
    env.create_file("ctx/ai.yaml", &ai_scope_content);
    fs::create_dir_all(env.full_path("doc")).unwrap();
    let arch_doc_content = fs::read_to_string("doc/refactor_architecture.md").unwrap();
    env.create_file("doc/refactor_architecture.md", &arch_doc_content);

    // 2. Create prompt and initial project files
    env.create_file("Makefile", "test:\n\t@cargo check\n");
    env.create_file(
        "my_retry_prompt.yml",
        r#"
        objective: "Add a public function `go()` to lib.rs"
        file_scoping:
          include:
            - "src/lib.rs"
            - "Cargo.toml"
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
    // Mock 1: The 'generate_markdown_plan' block
    let markdown_plan_response = json!({
        "candidates": [{
            "content": { "parts": [{ "text": "Plan: Add go() function." }], "role": "model" }
        }]
    });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "This is a creative step to think through the solution",
        )) // Unique to the markdown plan block
        .and(body_string_contains(
            "Add a public function `go()` to lib.rs",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(markdown_plan_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2: The 'generate_structured_tasks' block
    let structured_task_response = json!({
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("convert the provided markdown plan"))
        .respond_with(ResponseTemplate::new(200).set_body_json(structured_task_response))
        .mount(&env.mock_server)
        .await;

    // Mock 2.5: Finish the structured task generation turn
    let finish_response = json!({ "candidates": [{"content": { "parts": [{"text": "OK" }], "role": "model" }}] });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("create_task_list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response.clone()))
        .mount(&env.mock_server)
        .await;

    // Mock 3: The 'implement_tasks' block - FIRST attempt (with syntax error)
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "Your goal is to implement the current task.",
        )) // Unique to code.yml implement_tasks
        .and(body_string_contains("**Current Task:**"))
        .and(body_string_contains(
            "Add a public function `go()` to lib.rs",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response_fail))
        .mount(&env.mock_server)
        .await;

    // Mock 3.5: The model's response to the failed tool call. It should just be an empty ack.
    // The verification logic will trigger the retry.
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("edit_file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response.clone()))
        .mount(&env.mock_server)
        .await;

    // Mock 4: The 'implement_tasks' block - SECOND attempt (with fix)
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("The last attempt failed validation.")) // From on_failure_prompt
        .and(body_string_contains("expected `;`")) // From cargo check stderr
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response_success))
        .mount(&env.mock_server)
        .await;

    // Mock 4.5: The model's response to the successful tool call.
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .and(body_string_contains("edit_file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response))
        .mount(&env.mock_server)
        .await;

    // 4. Run the command
    let mut cmd = get_aide_cmd();
    env.apply_env(&mut cmd);
    cmd.current_dir(env.path());
    cmd.arg("run")
        .arg("code")
        .arg("--prompt")
        .arg("my_retry_prompt.yml");

    // 5. Assert success and file modification
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "Command failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Flow 'code' finished."),
        "Flow did not finish. Stderr:\n---\n{}\n---",
        stderr
    );
    assert!(stderr.contains("Verification failed for block 'implement_tasks'. Retrying"));

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("println!(\"go!\");"));

    let commit_msg = env.get_last_commit_message();
    assert_eq!(
        commit_msg,
        "aide-rs: auto-commit after task 'add-go-fn' in flow 'code'"
    );
}

#[tokio::test]
async fn test_e2e_plan_then_implement_flow() {
    let env = TestEnv::new().await;
    env.init_git_repo();

    // 1. Create flow and context files
    fs::create_dir_all(env.full_path("flows")).unwrap();
    let plan_flow_content = fs::read_to_string("flows/plan.yml").unwrap();
    env.create_file("flows/plan.yml", &plan_flow_content);
    let implement_flow_content = fs::read_to_string("flows/implement.yml").unwrap();
    env.create_file("flows/implement.yml", &implement_flow_content);

    fs::create_dir_all(env.full_path("ctx")).unwrap();
    let base_scope_content = fs::read_to_string("ctx/base.yaml").unwrap();
    env.create_file("ctx/base.yaml", &base_scope_content);

    // 2. Create prompt and initial project files
    env.create_file(
        "my_chained_prompt.yml",
        r#"
        objective: "Add a hello world function to lib.rs"
        file_scoping:
          include: ["src/lib.rs"]
        "#,
    );
    env.create_file(
        "Cargo.toml",
        r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#,
    );
    env.create_file("src/lib.rs", "// Initial content\n");

    // 3. Mock API call for 'plan' flow
    let plan_response_body = json!({
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "break it down into a high-level list of task descriptions",
        )) // Unique to plan.yml
        .respond_with(ResponseTemplate::new(200).set_body_json(plan_response_body))
        .mount(&env.mock_server)
        .await;

    // Mock the response after the tool call to finish the block
    let finish_response = json!({ "candidates": [{"content": { "parts": [{"text": "OK" }], "role": "model" }}] });
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response.clone()))
        .mount(&env.mock_server)
        .await;

    // 4. Run the 'plan' command
    let mut plan_cmd = get_aide_cmd();
    env.apply_env(&mut plan_cmd);
    plan_cmd.current_dir(env.path());
    plan_cmd
        .arg("run")
        .arg("plan")
        .arg("--prompt")
        .arg("my_chained_prompt.yml");

    let plan_output = plan_cmd.output().unwrap();
    assert!(
        plan_output.status.success(),
        "Plan command failed. Stderr: {}",
        String::from_utf8_lossy(&plan_output.stderr)
    );

    // Assert that the tasks file was created
    let log_dir = env.full_path(".ai/logs");
    let mut entries = fs::read_dir(log_dir).unwrap();
    let run_dir = entries.next().unwrap().unwrap().path();
    let tasks_path = run_dir.join(".ai/tasks.json");
    assert!(
        tasks_path.exists(),
        "tasks.json not found in log dir: {}",
        tasks_path.display()
    );
    let tasks_content = fs::read_to_string(&tasks_path).unwrap();
    assert!(tasks_content.contains("add-hello"));

    // 5. Mock API call for 'implement' flow
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
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains(
            "implement a single task from a pre-approved plan",
        )) // Unique to implement.yml
        .respond_with(ResponseTemplate::new(200).set_body_json(impl_response))
        .mount(&env.mock_server)
        .await;

    // Mock the response after the tool call to finish the block
    Mock::given(method("POST"))
        .and(path(
            "/v1beta/models/gemini-2.5-pro:generateContent",
        ))
        .and(body_string_contains("functionResponse"))
        .respond_with(ResponseTemplate::new(200).set_body_json(finish_response))
        .mount(&env.mock_server)
        .await;

    // 6. Run the 'implement' command
    let mut impl_cmd = get_aide_cmd();
    env.apply_env(&mut impl_cmd);
    impl_cmd.current_dir(env.path());
    impl_cmd
        .arg("run")
        .arg("implement")
        .arg("--prompt")
        .arg("my_chained_prompt.yml")
        .arg("--input-file")
        .arg(&tasks_path)
        .arg("--input-id")
        .arg("generate_tasks");

    let impl_output = impl_cmd.output().unwrap();
    assert!(
        impl_output.status.success(),
        "Implement command failed. Stderr: {}",
        String::from_utf8_lossy(&impl_output.stderr)
    );

    // 7. Assert success and file modification
    let stderr = String::from_utf8(impl_output.stderr).unwrap();
    assert!(
        stderr.contains("Flow 'implement' finished."),
        "Flow did not finish. Stderr:\n---\n{}\n---",
        stderr
    );

    let final_content = fs::read_to_string(env.full_path("src/lib.rs")).unwrap();
    assert!(final_content.contains("pub fn hello_world"));

    let commit_msg = env.get_last_commit_message();
    assert_eq!(
        commit_msg,
        "aide-rs: auto-commit after task 'add-hello' in flow 'implement'"
    );
}
