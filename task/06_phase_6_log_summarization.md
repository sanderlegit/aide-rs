# Phase 6: LLM-based Log Summarization

**Status: Complete**

## Goal
Implement a log summarization feature to manage context window limitations on verbose validation failures. This will involve a pre-processing step that uses a smaller, faster LLM to summarize long error messages before they are passed to the main implementation agent.

## Tasks
1.  **Update `GeminiClientWrapper`**:
    *   Add a new constructor or method to initialize a client for a smaller model (e.g., `gemini-1.5-flash`) specifically for summarization tasks. This might be a new `new_summarize_agent()` function.
2.  **Create Summarization Logic**:
    *   In `impl_agent.rs`, create a new async function `summarize_error(error_output: &str) -> Result<String>`.
    *   This function will:
        1.  Use the summarization Gemini client.
        2.  Construct a simple prompt: "Summarize this compiler/tool error into its most critical message: {error_output}".
        3.  Call the Gemini API.
        4.  Extract and return the summarized text from the response.
3.  **Integrate into `ImplAgent` Retry Loop**:
    *   In `impl_agent.rs`, inside the retry loop, when a validation or formatter command fails:
    *   Instead of passing the full error output directly into the `error_context`, call the new `summarize_error` function.
    *   Use the summarized error message to build the `error_context` for the next attempt's prompt.
    *   Add a configurable threshold (e.g., if error output > 1000 characters) to decide when to summarize, to avoid unnecessary API calls for short errors. This could be a new field in the `Config` struct.

## Test Coverage
*   **Integration Test for Summarization**:
    *   In `tests/e2e.rs` or a new test file, add a test for the `ImplAgent`'s error handling.
    *   Mock the Gemini API. The test will simulate a command failure with a very long error message.
    *   The mock server will have two expectations:
        1.  An initial call to the summarization model (`gemini-2.5-flash`) with the long error. It should respond with a short, summarized error.
        2.  A subsequent call to the implementation model (`gemini-2.5-pro`) where the prompt contains the *summarized* error, not the original long one.
    *   Assert that both API calls are made as expected.

## Completion Criteria
*   The `ImplAgent` successfully uses a smaller LLM to summarize long error messages before feeding them back into its correction prompt.
*   Integration tests verify that the summarization call happens correctly and the main prompt is constructed with the summarized error.
