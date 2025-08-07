Of course. Here is the complete and extended implementation specification for your Rust-based CLI agent. This document incorporates the feedback from our critical review and details all requested features, providing a comprehensive blueprint for development.

---

## **Implementation Specification: Rust AI Agent**

### **1. Overview & Core Philosophy**

This document specifies a Rust-based Command-Line Interface (CLI) tool designed for automated, agentic software development. The tool leverages the Google Gemini API, with a strict focus on its **Function Calling** capabilities to ensure structured, reliable interactions.

The core philosophy is built on three pillars:
1.  **Separation of Concerns:** The workflow is divided into two distinct, single-responsibility agents: a non-interactive `PlanAgent` for strategic planning and an iterative `ImplAgent` for tactical execution.
2.  **Robustness through Structure:** All significant interactions with the LLM will use Function Calling. This avoids fragile text parsing in favor of strongly-typed data, making the system more reliable and easier to debug.
3.  **Maintainability and Extensibility:** The architecture is modular, using traits and decoupled components to simplify maintenance and allow for the future addition of new agents and capabilities. The project will maximize code reuse by leaning on high-quality, industry-standard Rust libraries.

### **2. High-Level Architecture & Project Structure**

The application will be a library crate with a binary entry point. State will be managed via explicitly defined data models serialized to the filesystem, allowing for resumable workflows.

```plaintext
.
├── .ai/
│   ├── implementation_plan.json # Primary state file for workflows
│   └── prompts/                 # Directory for user-defined plan prompts
│       └── my_feature.toml
├── .aide-rs.toml                # Optional global configuration
├── .env                         # For GEMINI_API_KEY
├── Cargo.toml
└── src/
    ├── main.rs                  # CLI entry point: arg parsing, config setup, agent dispatch
    ├── lib.rs                   # Library root, exports key components
    ├── cli.rs                   # clap CLI definition (subcommands and arguments)
    ├── config.rs                # Configuration loading (file, env) and struct
    ├── error.rs                 # Custom application error types (ApiError, FileSystemError, etc.)
    ├── vcs.rs                   # Trait and implementation for Git operations (commit)
    ├── files.rs                 # File discovery using the `ignore` crate
    ├── gemini.rs                # Wrapper for gemini_client_rs; handles API calls, errors, history
    └── agents/
        ├── mod.rs               # Defines the generic `Agent` trait
        ├── plan_agent.rs        # Implementation of the PlanAgent
        ├── impl_agent.rs        # Implementation of the ImplAgent
        └── state.rs             # Core data models (PlanPrompt, Task, etc.)
```

### **3. Core Principles & Conventions**

*   **Error Handling:** The custom `error.rs` module will define a comprehensive `Error` enum using `thiserror`. It will distinguish between API errors, configuration errors, I/O errors, and validation failures to provide precise, user-friendly diagnostics.
*   **Coding Standards:** The project will adhere to standard Rust conventions. `cargo fmt` will be used for formatting, and `cargo clippy -- -D warnings` will be run as part of the CI pipeline to enforce high code quality.
*   **Logging:** The `tracing` crate will be used for structured logging. Log levels will be configurable via the CLI (`-v`, `-vv`) to control verbosity for debugging.

### **4. Command-Line Interface (CLI) Specification (`cli.rs`)**

The CLI will be defined using `clap`.

*   **`aide plan --prompt <PATH>`**
    *   **Description:** Invokes the `PlanAgent` to generate a structured implementation plan.
    *   **`--prompt <PATH>`** (Required): Path to a structured prompt file (e.g., `.ai/prompts/my_feature.toml`). This file must be in TOML format and deserialize into the `PlanPrompt` struct.
    *   **`--output-plan <PATH>`**: (Optional) Path to save the plan. Defaults to `.ai/implementation_plan.json`.

*   **`aide impl --plan <PATH>`**
    *   **Description:** Invokes the `ImplAgent` to execute a plan. The agent will automatically resume from the last failed or pending task.
    *   **`--plan <PATH>`** (Required): Path to an `implementation_plan.json` file.
    *   **`--max-retries <N>`**: (Optional) The maximum number of attempts per task. Defaults to `5`.
    *   **`--auto-commit`**: (Flag) If set, automatically creates a Git commit after all tasks are successfully completed.

### **5. State Management and Data Models (`agents/state.rs`)**

These structs define the core data flow and are serialized/deserialized to JSON for state management.

```rust
// In agents/state.rs
use serde::{Serialize, Deserialize};
use std::path::PathBuf;

// Input for the PlanAgent, deserialized from a TOML file.
#[derive(Serialize, Deserialize)]
pub struct PlanPrompt {
    pub objective: String, // A high-level description of the goal. Can be a multiline string in TOML.
    pub file_scoping: FileScope,
    pub coding_conventions: String, // A detailed description of coding standards. Can be a multiline string in TOML.
    pub formatter_command: Option<String>,
    pub validation_commands: Vec<ValidationStep>,
}

// Defines file include/exclude rules.
#[derive(Serialize, Deserialize)]
pub struct FileScope {
    pub include: Vec<String>, // Glob patterns
    pub exclude: Vec<String>, // Glob patterns
}

// The primary state file for a workflow.
#[derive(Serialize, Deserialize)]
pub struct ImplementationPlan {
    pub original_prompt: PlanPrompt,
    pub tasks: Vec<Task>,
}

// A single, executable task within a plan.
#[derive(Serialize, Deserialize, Clone)]
pub struct Task {
    pub description: String,
    pub file_scoping: FileScope,
    pub validation_steps: Vec<ValidationStep>,
    pub status: TaskStatus,
    pub attempts: u32,
    pub result: Option<TaskResult>, // Populated on success
}

// The outcome of a successfully completed task.
#[derive(Serialize, Deserialize, Clone)]
pub struct TaskResult {
    pub success: bool,
    pub agent_tips: String, // The agent's explanation of the fix.
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Success,
    Failed,
}

// A command to be run for validation.
#[derive(Serialize, Deserialize, Clone)]
pub struct ValidationStep {
    pub command: String,
    pub expected_exit_code: i32,
}
```

### **6. Detailed Component Breakdown**

#### **6.1. PlanAgent (`agents/plan_agent.rs`)**

*   **Trait Impl:** `impl Agent<Input = PlanPrompt, Output = ImplementationPlan>`
*   **Logic:**
    1.  Receives a `PlanPrompt`.
    2.  Uses `files.rs` to get a file list based on the prompt's `FileScope`.
    3.  Constructs a system prompt for the "Expert Software Architect" persona.
    4.  Defines a Gemini Function Declaration: `create_implementation_plan(tasks: Vec<Task>)`.
    5.  The user prompt will be a formatted string containing the objective, file list, coding conventions, and validation commands, ending with: "Generate a detailed implementation plan by calling the `create_implementation_plan` function."
    6.  The agent calls the Gemini API. The response must be a function call.
    7.  It deserializes the function arguments into an `ImplementationPlan`, setting the status of each new task to `Pending`.
    8.  The plan is saved to the specified output file.

#### **6.2. ImplAgent (`agents/impl_agent.rs`)**

*   **Trait Impl:** `impl Agent<Input = PathBuf, Output = ()>` (Input is the path to the plan file).
*   **Logic:**
    1.  Loads and deserializes the `ImplementationPlan` from the given path.
    2.  Iterates through each `Task` in `plan.tasks`.
    3.  **For each task:**
        a. If `task.status == TaskStatus::Success`, skip it and print a status message.
        b. **Begin Retry Loop:** `for attempt in 0..max_retries`
            i.   Read the contents of all files specified in the *task's* `FileScope` using `files.rs`.
            ii.  Define Gemini Function Declarations for code modification:
                 *   `edit_file(path: String, new_content: String)`
                 *   `create_file(path: String, content: String)`
            iii. **Construct Prompt Context:**
                 *   **System Prompt:** "You are an expert pair programmer. Implement the user's request by calling the provided file manipulation functions. Adhere strictly to the coding conventions provided. After your final edit, run the formatter if one is specified. Finally, explain the problem and your solution."
                 *   **Context:** The task description, coding conventions, and full content of all scoped files.
                 *   **Correction Context (if `attempt > 0`):** "On the previous attempt, validation failed. The command `{cmd}` exited with code `{code}`. Output: `{stdout/stderr}`. Please analyze the error, fix the code, and explain the fix."
            iv.  **Call Gemini API**. Process the function calls to apply file edits.
            v.   **Run Formatter:** If `plan.original_prompt.formatter_command` is `Some`, execute it. If it fails, treat it as a validation failure and add its output to the correction context for the next loop.
            vi.  **Run Validation:** Sequentially execute each `ValidationStep`.
            vii. **Check Success:** If a command's exit code does not match `expected_exit_code`, the step fails. Capture its output, increment `task.attempts`, and `continue` to the next retry loop iteration.
            viii.If all validation steps pass: update `task.status` to `Success`, populate `task.result` with the agent's tips, save the entire `ImplementationPlan` back to disk, and `break` the retry loop.
        c. If the retry loop finishes without success, set `task.status` to `Failed` and save the plan. The entire process halts, informing the user which task failed.
    4.  **Finalize:** If all tasks succeed and `--auto-commit` is set, use `vcs.rs` to create a Git commit with a message generated from the task descriptions.

#### **6.3. File Filtering (`files.rs`)**

*   **Dependencies:** `ignore` and `globset`.
*   **Function:** `pub fn get_filtered_files(base_dir: &Path, scope: &FileScope) -> Result<Vec<PathBuf>>`
*   **Logic:** Uses the `ignore` crate to build a `WalkBuilder`. It uses `globset` to efficiently match files against the include/exclude patterns, while leveraging the `ignore` library's built-in support for `.gitignore` and parallel directory traversal.

### **7. Testing Strategy**

*   **Unit Testing:**
    *   **Scope:** Test individual functions in isolation.
    *   **Examples:** Test the `files.rs` module against a temporary directory structure. Test the `config.rs` loading logic.
    *   **Tools:** Standard `#[test]` attribute, `tempfile` crate.

*   **Integration Testing:**
    *   **Scope:** Test the interaction between internal components, mocking external services.
    *   **Examples:** Test the `PlanAgent` and `ImplAgent` logic.
    *   **Tools:**
        *   `wiremock` crate to run a mock Gemini API server. This is **critical**. The mock server will serve pre-canned, expected function call responses when it receives a specific prompt, allowing tests to verify agent logic without API costs or non-determinism.
        *   Test agent workflows by asserting that the agent sends the correct prompts and correctly interprets the mock server's function call responses.

*   **End-to-End (E2E) Testing:**
    *   **Scope:** Test the compiled binary from a user's perspective.
    *   **Examples:** Run a full `plan` then `impl` cycle on a small sample project.
    *   **Tools:**
        *   `assert_cmd` to execute the binary and assert on exit codes and output streams.
        *   `tempfile` to create temporary git repositories for the tests to run in.
        *   These tests will also use the `wiremock`ed Gemini server to ensure they are deterministic and can run in CI.

### **8. Key Implemented Features**

*   **LLM-based Log Summarization:** To manage context window limitations on verbose validation failures, the `ImplAgent` includes a pre-processing step for errors. On a failed validation with a long error message, before constructing the correction context for a retry, it makes a separate call to a smaller model (Gemini Flash) with a prompt to "Summarize this compiler error into its most critical message." The summarized error is then used in the main agent's prompt, reducing token count and focusing the agent on the core issue.
