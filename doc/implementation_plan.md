# Implementation Plan for Architecture Refactor

This document outlines the remaining tasks to complete the transition to the declarative, flow-based architecture. The goal is to restore and then exceed the previous functionality in a more robust and flexible way.

## Phase 1: Core Logic Implementation (Current Focus)

This phase focuses on building out the core components of the `FlowRunner` so that it can execute a simple, single-block flow from start to finish.

-   **[X] Task 1.1: Implement Prompt Construction (`src/prompt.rs`)**
    -   Create a `PromptBuilder` struct.
    -   Implement logic to process a `Prompt` definition from `types.rs`.
    -   Handle `static_text`, `prompt_file_field`, and `previous_output` composition types.
    -   Implement `file_contents` logic:
        -   Load and merge named scopes from `ctx/*.yml`.
        -   Load scope from the user's TOML prompt file.
        -   Use the `files.rs` module to get the final list of files.
        -   Read file contents and format them into a single string.
    -   **Unit Tests**: Add tests for each composition type and for scope merging.

-   **[X] Task 1.2: Implement Tool Handling (`src/tools.rs`)**
    -   Define a `Tool` trait and a `ToolExecutor` struct.
    -   Create concrete tool structs (e.g., `FileSystemTool`, `DocRetrieverTool`, `TaskCreatorTool`).
    -   Implement logic to generate `FunctionDeclaration` schemas for enabled tools.
    -   Implement logic to parse a `FunctionCall` from the Gemini API and dispatch it to the correct tool.
    -   **Unit Tests**: Test schema generation and tool dispatch.

-   **[X] Task 1.3: Flesh out the `FlowRunner` (`src/runner.rs`)**
    -   Integrate `PromptBuilder` and `ToolExecutor`.
    -   Implement the main loop to iterate through `blocks` in a `Flow`.
    -   For each block:
        -   Build the prompt.
        -   Call the `GeminiClientWrapper`.
        -   Handle the response: check for function calls and dispatch to the `ToolExecutor`.
        -   Store the block's output (text or tool result) in a `HashMap` keyed by `block.id`.
    -   Implement history management (`full`, `none`, `last_n`).

## Phase 2: Advanced Flow Control & Verification

This phase implements the more complex features of the architecture, such as looping and self-correction.

-   **[X] Task 2.1: Implement Block Verification**
    -   Add logic to the `FlowRunner` to handle the `verification` block property.
    -   Implement the `command` verification strategy:
        -   Execute the shell command.
        -   Check the exit code.
        -   On failure, construct the `on_failure_prompt` and re-run the block.
    -   **[X]** Implement the `prompt` verification strategy.

-   **[X] Task 2.2: Implement Looping for Task Lists**
    -   Add special handling in the `FlowRunner` for blocks that operate on a list output from a previous block (e.g., implementing a `TaskList`).
    -   This involves iterating over the list and running the block's logic for each item.
    -   The current loop item is exposed to the prompt system via the `as` key in the `looping` config.
    -   This is the mechanism for the "Just-in-Time Planning" described in the architecture.

## Phase 3: Testing and Validation

This phase ensures the new system is working correctly end-to-end.

-   **[~] Task 3.1: Restore Unit Tests**
    -   Go through all `#[cfg(test)]` modules and update them to reflect the new architecture.
    -   **[X]** Add unit tests for `prompt.rs`.
    -   **[X]** Add unit tests for `tools.rs`.
    -   **[ ]** Add unit tests for `runner.rs` (to be covered via E2E tests).

-   **[~] Task 3.2: Create E2E Test Suite (`tests/e2e.rs`)**
    -   Create a new test repository or project within the `tests/` directory.
    -   Write E2E tests for the primary flows (`plan`, `code`).
    -   Use `wiremock` to mock Gemini API calls.
    -   **[X] Test Case: `plan` flow**:
        -   Run `aide-rs run plan --prompt ...`.
        -   Mock the API call.
        -   Assert that the correct structured `TaskList` is produced.
    -   **[X] Test Case: `code` flow (single task)**:
        -   Run `aide-rs run code --prompt ...`.
        -   Mock the sequence of API calls (plan, implement, verify).
        -   Assert that the target file is modified correctly.
    -   **[X] Test Case: `code` flow with verification failure and retry**:
        -   Mock an initial implementation that fails `cargo check`.
        -   Mock the follow-up API call that includes the error message.
        -   Mock the final, correct implementation.
        -   Assert the file is eventually correct.

-   **[ ] Task 3.3: Implement JIT Planning in `code` flow**
    -   Refactor the `FlowRunner` to support executing multiple blocks within a single loop iteration.
    -   Update `code.yml` to have separate `jit_plan` and `execute_plan` blocks within the implementation loop.
    -   Add a new tool to create a `DetailedTaskPlan`.
    -   Add an E2E test for the full JIT planning flow.
