# Architecture Overview

This document provides a high-level overview of the `aide-rs` architecture, detailing the flow of data and the responsibilities of each major component.

The application follows a two-phase workflow, orchestrated by two distinct agents:

1.  **Plan Phase**: A `PlanAgent` analyzes a user's objective and the existing codebase to generate a structured, step-by-step implementation plan.
2.  **Implement Phase**: An `ImplAgent` executes the tasks defined in the plan, iteratively making code changes, running validation, and handling errors until the objective is complete.

This separation of concerns makes the process robust and debuggable. The state is managed explicitly in a TOML file (e.g., `.ai/plan_my_feature_1678886400.toml`), allowing workflows to be inspected and resumed.

## Core Components and Flow

The application's logic is orchestrated from `src/main.rs`, which parses command-line arguments and dispatches to the appropriate agent.

### 1. CLI and Entrypoint (`src/main.rs`, `src/cli.rs`)

-   **`cli.rs`**: Defines the command-line interface using `clap`. It specifies the `plan` and `impl` subcommands and their arguments.
-   **`main.rs`**: The application entry point. It parses the CLI input and orchestrates the high-level workflow:
    - For `aide plan`, it deserializes the user's TOML prompt, runs the `PlanAgent`, and saves the resulting plan.
    - For `aide impl`, it loads a plan and runs the `ImplAgent` to execute it.

### 2. State Management (`src/agents/state.rs`)

This module defines the core data structures that represent the application's state. All structs derive `serde::Serialize` and `Deserialize` for easy conversion to/from JSON and TOML.

-   **`PlanPrompt`**: The initial input for the `plan` phase, containing the user's high-level objective, file scope, and validation rules.
-   **`ImplementationPlan`**: The central state object. It contains the original prompt and a `Vec<Task>`. It is saved to a uniquely named TOML file in the `.ai/` directory.
-   **`Task`**: A single, atomic step in the plan. It includes a description, validation steps, and its current `status` (`Pending`, `Success`, `Failed`).

### 3. The `Agent` Trait (`src/agents/mod.rs`)

A generic `async` trait that defines a common interface for all agents:

```rust
pub trait Agent {
    type Input;
    type Output;
    async fn run(&self, input: Self::Input) -> Result<Self::Output>;
}
```

This abstraction standardizes how agents are invoked.

### 4. The `PlanAgent` (`src/agents/plan_agent.rs`)

-   **Responsibility**: To convert a high-level `PlanPrompt` into a detailed `ImplementationPlan`.
-   **Flow**:
    1.  Receives the `PlanPrompt`.
    2.  If `use_google_search_for_deps` is true, it first performs a Google Search-enabled API call to research libraries. The results are injected into the main prompt.
    3.  Uses `files::get_filtered_files` to gather the file context.
    4.  **Step 1: Generate Task Descriptions**: It constructs a prompt asking the Gemini API to act as an architect and break the objective into a high-level list of task descriptions. It provides a `create_task_descriptions` function declaration.
    5.  **Step 2: Detail Each Task**: It iterates through the descriptions from the previous step. For each description, it constructs a new prompt asking the API to define the specific `validation_steps` for that task. It provides a `create_task_details` function declaration.
    6.  The agent calls the Gemini API for each step, deserializing the function call arguments.
    7.  The agent assembles the final `ImplementationPlan` from the collected details and returns it.

### 5. The `ImplAgent` (`src/agents/impl_agent.rs`)

-   **Responsibility**: To execute the tasks in an `ImplementationPlan`.
-   **Flow**:
    1.  Loads the `ImplementationPlan` from its file path.
    2.  Iterates through each `Task`, skipping those already marked as `Success`.
    3.  **Retry Loop**: For each task, it enters a retry loop (`max_retries`).
        a. **Prompt Construction**: It gathers the content of files defined in the *original prompt's* global `file_scoping` and constructs a prompt for the Gemini API. If it's a retry, it includes the error message from the previous failed attempt.
        b. **Tool Definition**: It provides `edit_file` and `create_file` function declarations to the API.
        c. **API Call**: It calls the Gemini API, which returns function calls to modify the filesystem. The agent applies these changes.
        d. **Validation**: It runs the formatter (if configured) and all `validation_steps` for the task.
        e. **Success/Failure**:
            - If validation succeeds, the task is marked `Success` and the plan on disk is updated. If `--auto-commit` is enabled, a Git commit is created for the task before moving to the next one.
            - If validation fails, the error output is captured. If the error is long, it's summarized via another API call (`summarize_error`). If the `--enrich-errors` flag is active, the agent will also use Google Search to find relevant documentation for the error. The error context (potentially enriched) is saved for the next attempt in the retry loop.

### 6. Supporting Modules

-   **`gemini.rs` (`GeminiClientWrapper`)**: A wrapper that handles all communication with the Google Gemini REST API. It constructs requests, sends them via `reqwest`, and processes responses, including handling function calling tools. It provides constructors for different agent types (plan, impl, summarize) which may use different models.
-   **`files.rs`**: Provides the `get_filtered_files` utility, which uses the `ignore` crate to walk the directory tree and find files matching a `FileScope`, while respecting `.gitignore` rules.
-   **`vcs.rs`**: Provides Git functionality, specifically `add_and_commit`, using the `git2` crate.
-   **`error.rs`**: Defines a centralized `Error` enum using `thiserror` for clean, consistent error handling across the application.
