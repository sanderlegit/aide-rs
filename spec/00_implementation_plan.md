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
│   ├── plan_my_feature_1678886400.toml # Example of a primary state file for a workflow
│   └── prompts/                        # Directory for user-defined plan prompts
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
    *   **`--output-plan <PATH>`**: (Optional) Path to save the plan. If not provided, a unique name will be generated (e.g., `.ai/plan_my_feature_1678886400.toml`).

*   **`aide impl --plan <PATH>`**
    *   **Description:** Invokes the `ImplAgent` to execute a plan. The agent will automatically resume from the last failed or pending task.
    *   **`--plan <PATH>`** (Required): Path to an implementation plan TOML file.
    *   **`--max-retries <N>`**: (Optional) The maximum number of attempts per task. Defaults to `5`.
    *   **`--auto-commit`**: (Flag) If set, automatically creates a Git commit after each task is successfully completed.
    *   **`--enrich-errors`**: (Flag) If set, allows the agent to use the `doc-retriever` tool to look up documentation for types and traits related to a compilation error.

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
    pub modified_files: Vec<String>,
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
*   **Logic:** The `PlanAgent` operates in a two-step process to build a detailed plan.
    1.  Receives a `PlanPrompt`.
    2.  Uses `files.rs` to get a file list based on the prompt's `FileScope`.
    3.  **Step 1: Generate Task Descriptions.**
        *   Constructs a prompt for an "Expert Software Architect" persona, asking it to break down the user's objective into a high-level list of task descriptions.
        *   Defines a Gemini Function Declaration: `create_task_descriptions(tasks: Vec<String>)`.
        *   Calls the Gemini API and deserializes the resulting function call to get a list of strings.
    4.  **Step 2: Detail Each Task.**
        *   Iterates through the list of descriptions from the previous step.
        *   For each description, it creates a new prompt asking the architect to define the specific `validation_steps` for that single task.
        *   Defines a Gemini Function Declaration: `create_task_details(validation_steps: Vec<ValidationStep>)`.
        *   Calls the Gemini API and deserializes the function call arguments.
    5.  **Step 3: Assemble Plan.**
        *   Combines the descriptions and their corresponding details into a final `ImplementationPlan`.
        *   The plan is saved to the specified output file.

#### **6.2. ImplAgent (`agents/impl_agent.rs`)**

*   **Trait Impl:** `impl Agent<Input = PathBuf, Output = ()>` (Input is the path to the plan file).
*   **Logic:**
    1.  Loads and deserializes the `ImplementationPlan` from the given path.
    2.  Iterates through each `Task` in `plan.tasks`.
    3.  **For each task:**
        a. If `task.status == TaskStatus::Success`, skip it and print a status message.
        b. **Begin Retry Loop:** `for attempt in 0..max_retries`
            i.   Read the contents of all files specified in the *original prompt's* global `FileScope` using `files.rs`.
            ii.  Define Gemini Function Declarations for code modification:
                 *   `edit_file(path: String, new_content: String)`
                 *   `create_file(path: String, content: String)`
            iii. **Construct Prompt Context:**
                 *   **System Prompt:** "You are an expert pair programmer. Your goal is to fix a compilation error. Analyze the error and the provided code. If you are unsure about an API, use the `doc_retriever` tool to get documentation. You can call it multiple times. Once you have enough information, call the file manipulation functions to fix the code. Finally, explain the problem and your solution."
                 *   **Context:** The task description, coding conventions, and full content of all scoped files.
                 *   **Correction Context (if `attempt > 0`):** "On the previous attempt, validation failed. The command `{cmd}` exited with code `{code}`. Output: `{stdout/stderr}`. Please analyze the error, fix the code, and explain the fix."
            iv.  **Begin Tool-Use Loop:**
                 *   Call the Gemini API. The agent can respond with a request to use a tool (`doc_retriever`, `edit_file`, `create_file`).
                 *   If `doc_retriever` is called, the `ImplAgent` executes it locally, captures the JSON output, and sends it back to the LLM as a `FunctionResponse`. This conversational loop continues until the LLM has enough information.
                 *   If `edit_file` or `create_file` is called, the agent applies the changes and the tool-use loop terminates for this attempt.
            v.   **Run Formatter:** If `plan.original_prompt.formatter_command` is `Some`, execute it. If it fails, treat it as a validation failure and add its output to the correction context for the next loop.
            vi.  **Run Validation:** Sequentially execute each `ValidationStep`.
            vii. **Check Success:** If a command's exit code does not match `expected_exit_code`, the step fails. Capture its output. Increment `task.attempts`, and `continue` to the next retry loop iteration with the new error context.
            viii.If all validation steps pass: update `task.status` to `Success`, populate `task.result` with the agent's tips, save the entire `ImplementationPlan` back to disk, and `break` the retry loop.
        c. If the retry loop finishes without success, set `task.status` to `Failed` and save the plan. The entire process halts, informing the user which task failed.
    4.  **Finalize:** After the loop, the agent's work is complete. If `--auto-commit` was used, the changes will have been committed incrementally.

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

*   **LLM-based Documentation Retrieval:** To fix complex compiler errors, the `ImplAgent` can engage in a multi-turn dialogue with the LLM. When the `--enrich-errors` flag is active, the agent provides a `doc-retriever` tool. The LLM can call this tool to get structured documentation for specific types, traits, or modules from the project's dependencies. The agent runs the tool locally and sends the documentation back to the LLM, allowing it to gather information before proposing a fix.
