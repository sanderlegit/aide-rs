# Phase 4: Agent Implementation and API Integration

**Status: Complete**

## Goal
Build the core intelligent components of the application: the `PlanAgent` and `ImplAgent`. This involves wrapping the Gemini API client and implementing the primary logic loops for planning and execution.

## Tasks
1.  **Define Agent Trait**: In `src/agents/mod.rs`, define a generic `Agent` trait to establish a common interface for agents.
2.  **Implement Gemini Wrapper (`gemini.rs`)**:
    *   Create `src/gemini.rs` to encapsulate all interactions with the `gemini_client_rs` library.
    *   This wrapper will be responsible for:
        *   Initializing the client (e.g., loading the API key from `.env`).
        *   Constructing `GenerateContentRequest` objects, including system prompts, user messages, and tool definitions (function declarations).
        *   Handling responses and extracting function calls or text content.
        *   Translating `gemini_client_rs` errors into the application's custom `Error` type.
3.  **Implement `PlanAgent` (`agents/plan_agent.rs`)**:
    *   Implement the `Agent` trait for `PlanAgent`.
    *   The agent's main logic will:
        1.  Receive a `PlanPrompt`.
        2.  Use `files.rs` to list scoped files.
        3.  Construct a detailed prompt for the Gemini API, asking it to call the `create_implementation_plan` function.
        4.  Call the Gemini API via the `gemini.rs` wrapper.
        5.  Deserialize the function call arguments into an `ImplementationPlan` struct.
        6.  Save the plan to a JSON file.
4.  **Implement `ImplAgent` (`agents/impl_agent.rs`)**:
    *   Implement the `Agent` trait for `ImplAgent`.
    *   The agent's main logic will be a loop over the tasks in an `ImplementationPlan`:
        1.  For each task, enter a retry loop.
        2.  Read scoped files and construct a prompt including the task, file contents, and any error context from previous failed attempts.
        3.  Define and provide the `edit_file` and `create_file` function declarations to the Gemini API.
        4.  Call the API and apply the file modifications returned in the function calls.
        5.  Run formatter and validation commands.
        6.  On success, update task status and save the plan. On failure, record the error and continue the retry loop.

## Test Coverage
*   **Integration Tests with Mocking**:
    *   Use the `wiremock` crate to create a mock HTTP server that simulates the Gemini API.
    *   **For `gemini.rs`**: Test that the wrapper sends correctly formatted JSON requests to the mock server.
    *   **For `PlanAgent`**: Write a test that runs the agent, asserting that it sends the expected prompt to `wiremock` and correctly processes the pre-canned function call response to create a plan file.
    *   **For `ImplAgent`**: Write a test that runs the agent against a task. The mock server will return a sequence of function calls to edit files. The test will assert that the agent applies the edits correctly and runs the (mocked) validation commands.

## Completion Criteria
*   All integration tests for the agents pass.
*   `PlanAgent` can generate a valid `implementation_plan.json` based on a mocked API response.
*   `ImplAgent` can successfully execute a task list against a mocked file system and API, applying changes and validating them.
