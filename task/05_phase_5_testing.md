# Phase 5: End-to-End (E2E) Testing

**Status: Complete**

## Goal
Verify that the compiled application works correctly from a user's perspective by testing the entire workflow from command-line invocation to final output. These tests ensure all components are integrated correctly.

## Tasks
1.  **Set up Test Environment**:
    *   Create a `tests/` directory for E2E tests.
    *   Each test will use `tempfile` to create an isolated directory, which will act as a temporary project root.
    *   A single `wiremock` server will be shared across tests to mock the Gemini API.
2.  **E2E Test for `aide plan`**:
    *   Create a test function `test_plan_workflow`.
    *   Inside the temporary directory, create a sample `my_feature.toml` prompt file.
    *   Configure the `wiremock` server to expect a specific prompt for plan generation and respond with a valid, pre-canned `create_implementation_plan` function call.
    *   Use `assert_cmd` to run the command: `aide plan --prompt my_feature.toml`.
    *   Assert that the command exits with a success code.
    *   Assert that a uniquely named plan file (e.g., `.ai/plan_my_feature_123.toml`) is created and its contents match the expected plan derived from the mock response.
3.  **E2E Test for `aide impl`**:
    *   Create a test function `test_impl_workflow`.
    *   Inside the temporary directory, create a sample project structure (e.g., a `src/main.rs` file) and a plan TOML file that describes a change to that file.
    *   Configure `wiremock` to expect a prompt for code modification and respond with an `edit_file` function call.
    *   Use `assert_cmd` to run `aide impl --plan <path_to_plan.toml>`.
    *   Assert that the command exits with a success code.
    *   Assert that the `src/main.rs` file has been modified as specified in the mock response.
4.  **E2E Test for `aide impl --auto-commit`**:
    *   Extend the `test_impl_workflow` or create a new test.
    *   Initialize a Git repository in the temporary directory.
    *   Run the `aide impl` command with the `--auto-commit` flag.
    *   After the command succeeds, use the `git2` crate to open the repository and assert that a new commit has been created with a message derived from the plan's tasks.

## Test Coverage
*   This phase provides comprehensive coverage for the primary user stories:
    *   Generating a plan from a prompt.
    *   Executing a plan to modify code.
    *   Automatically committing successful work.

## Completion Criteria
*   `cargo test` runs all unit, integration, and E2E tests successfully.
*   The application is deemed feature-complete and robust, with its main workflows validated under controlled, deterministic conditions.
