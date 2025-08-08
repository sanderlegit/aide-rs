// use aide_rs::vcs::add_and_commit;
use assert_cmd::Command;
// use git2::Repository;
// use serde_json::json;
// use std::fs;
use std::path::PathBuf;
// use wiremock::matchers::{body_string_contains, method, path_regex};
// use wiremock_logical_matchers::not;
// use wiremock::{Mock, ResponseTemplate};

mod common;
// use common::TestEnv;

#[allow(dead_code)]
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

// TODO: All E2E tests need to be rewritten for the new flow-based architecture.
// The general approach will be:
// 1. Create a TestEnv.
// 2. Create a sample project structure (e.g., `src/main.rs`, `Cargo.toml`).
// 3. Create a `my_prompt.toml` file with an objective.
// 4. Create a `flows/my_test_flow.yml` that defines the steps for the test.
// 5. Mock the sequence of expected Gemini API calls and their responses using `wiremock`.
// 6. Run `get_aide_cmd().arg("run").arg("my_test_flow").arg("--prompt").arg("my_prompt.toml")`.
// 7. Assert that the command succeeds.
// 8. Assert that files were modified correctly.
// 9. Assert that any expected side-effects (like git commits) occurred.

#[test]
fn placeholder_for_new_e2e_tests() {
    // This is a placeholder to ensure the test suite compiles.
    // It should be replaced with actual E2E tests for the new architecture.
    assert!(true);
}
