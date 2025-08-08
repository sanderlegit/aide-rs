# Architectural Review: `aide-rs`

This document provides an architectural review of the `aide-rs` command-line tool. It analyzes the program's structure, its operational workflow, and the design of its core agent-based system.

## 1. High-Level Overview

`aide-rs` is a Rust-based AI agent designed for automated software development. Its architecture is centered on a **two-phase, agentic workflow** that promotes a strong separation of concerns:

1.  **Planning Phase**: A strategic `PlanAgent` analyzes a high-level user objective and the existing codebase to generate a structured, step-by-step implementation plan. This agent acts as an "architect."
2.  **Implementation Phase**: A tactical `ImplAgent` executes the plan task-by-task. It writes and modifies code, runs validation commands (like tests and linters), and uses a retry loop with error analysis to self-correct. This agent acts as a "pair programmer."

The system's core philosophy relies on structured, predictable interactions with the Google Gemini LLM via its **Function Calling** feature. This avoids fragile text parsing in favor of strongly-typed data, making the system more robust and debuggable. State is managed explicitly on the filesystem in TOML files, allowing workflows to be inspected, audited, and resumed.

## 2. Code Structure and Modularity

The project is organized into a library crate with a binary entry point, following standard Rust conventions. The modular design ensures that components are decoupled and single-purpose.

-   **`src/main.rs`: Entrypoint & Orchestration**
    The `main.rs` file is the application's entry point. It initializes the `tracing` logger, parses command-line arguments using the `Cli` struct, and dispatches control to the appropriate agent based on the subcommand (`plan` or `impl`). It acts as the high-level orchestrator for the entire workflow.

-   **`src/cli.rs`: Command-Line Interface**
    This module defines the CLI structure using `clap`'s derive macros. It specifies the `plan` and `impl` subcommands and their respective arguments (e.g., `--prompt`, `--plan`, `--auto-commit`, `--enrich-errors`), providing the primary user-facing interface.

-   **`src/agents/state.rs`: State Management & Data Models**
    This is a critical module that defines the core data structures for state management. All structs (`PlanPrompt`, `ImplementationPlan`, `Task`, etc.) derive `serde::Serialize` and `Deserialize`, allowing them to be easily converted to and from TOML for persistence. This module effectively defines the application's "database schema."

-   **`src/agents/mod.rs`: The Generic `Agent` Trait**
    This module defines the generic `Agent` trait, which establishes a common, asynchronous interface for all agents. This abstraction is key to the system's extensibility, as it provides a standardized contract for how agents are invoked.

-   **`src/gemini.rs` & `src/gemini_types.rs`: Gemini API Interaction**
    -   `gemini_types.rs` contains the Rust struct definitions that map directly to the Gemini API's JSON request and response schemas. This ensures type-safe, structured communication with the LLM.
    -   `gemini.rs` provides the `GeminiClientWrapper`, a facade over `reqwest` that handles all HTTP communication with the Gemini API. It constructs requests, sends them, processes responses, logs all interactions via the `RunLogger`, and translates API or network errors into the application's custom `Error` type. It includes factory methods (`new_plan_agent`, `new_impl_agent`) that can configure different models for different purposes.

-   **`src/logging.rs`: Structured Logging**
    The `RunLogger` provides comprehensive, run-specific logging. For each execution, it creates a unique timestamped directory in `.ai/logs/` containing:
    1.  `summary.log`: A human-readable log of major events.
    2.  `complete.log.jsonl`: A machine-readable JSONL file with detailed, structured data for every event, which is invaluable for debugging.

-   **Supporting Modules**
    -   `error.rs`: Defines a centralized `Error` enum using `thiserror` for clean, consistent error handling.
    -   `files.rs`: Provides the `get_filtered_files` utility, which uses the `ignore` crate to find files matching a `FileScope` while respecting `.gitignore`.
    -   `vcs.rs`: Encapsulates Git operations (specifically `add_and_commit`) using the `git2` crate.
    -   `config.rs`: A placeholder for loading global configuration, demonstrating foresight for future expansion.

## 3. How It Works: The Execution Loop

The application's workflow is driven by the user's commands and configured through TOML files and environment variables.

### Configuration

-   **Environment Variables**: `GEMINI_API_KEY` is required and loaded from a `.env` file.
-   **Prompt File (`.toml`)**: This is the primary configuration for a run. It defines the `objective`, `file_scoping` rules, `coding_conventions`, and `validation_commands` that guide the agents.
-   **CLI Flags**: Flags like `--auto-commit` and `--enrich-errors` allow users to modify the `ImplAgent`'s behavior at runtime.

### The `plan` Command Workflow

1.  The user runs `aide-rs plan --prompt <path_to_prompt.toml>`.
2.  The `PlanAgent` is invoked with the deserialized `PlanPrompt`.
3.  The agent executes a **two-step process**:
    a. **Generate Descriptions**: It calls the Gemini API with a prompt and a `create_task_descriptions` function definition, asking it to break the objective into a high-level list of task descriptions.
    b. **Detail Tasks**: It iterates through the descriptions, calling the Gemini API for each one with a `create_task_details` function definition to determine the appropriate validation steps.
4.  The agent assembles the final `ImplementationPlan` and saves it to a new TOML file in the `.ai/` directory.

### The `impl` Command Workflow

1.  The user runs `aide-rs impl --plan <path_to_plan.toml>`.
2.  The `ImplAgent` loads the `ImplementationPlan`.
3.  It begins its main loop, iterating through each `Task` in the plan.
4.  For each task, it enters a **retry loop** (controlled by `--max-retries`).
5.  Inside the retry loop, it enters a **tool-use loop** for multi-turn conversation with the LLM.
    a. It constructs a detailed prompt containing the task, file context, and any error messages from the previous failed attempt.
    b. It calls the Gemini API, providing tools like `edit_file`, `create_file`, and (if `--enrich-errors` is enabled) `doc_retriever`.
    c. The `handle_function_call` method executes the tool requested by the LLM. If a file is modified, the tool-use loop terminates for this attempt. If documentation is retrieved, the information is sent back to the LLM in the next turn of the loop.
6.  After the tool-use loop, the agent runs the formatter and all `validation_steps`.
7.  **On Success**: The task is marked `Success`, the plan file is updated, and (if `--auto-commit` is on) a Git commit is created. The agent moves to the next task.
8.  **On Failure**: The error is recorded, and the next iteration of the **retry loop** begins with the new error context. If all retries fail, the entire process halts.

## 4. Agent Design and Extensibility

The agent-based design is the core of the application's intelligence and a key strength of its architecture.

### The `PlanAgent`: The Architect

-   **Role**: Strategic planning.
-   **Logic**: A simple, two-step sequential process.
-   **Tools**: Uses function calling to *describe* work (`create_task_descriptions`, `create_task_details`).
-   **State**: Stateless within a single run; its sole output is the `ImplementationPlan`.

### The `ImplAgent`: The Implementer

-   **Role**: Tactical execution and self-correction.
-   **Logic**: A complex, nested loop structure (tasks -> retries -> tool-use).
-   **Tools**: Uses function calling to *perform* work (`edit_file`, `create_file`) and *research* problems (`doc_retriever`).
-   **State**: Highly stateful; it continuously reads and writes the `ImplementationPlan` to disk to track progress.

### Comparison and Synergy

The two agents are distinct but complementary. The `PlanAgent` provides a high-quality, structured input that constrains the `ImplAgent`, allowing it to focus on smaller, more manageable problems. While both implement the same `Agent` trait and use the `GeminiClientWrapper`, their internal logic, tools, and goals are fundamentally different, reflecting their specialized roles.

### Creating New Agents: A Path to Extensibility

The architecture is highly extensible and well-suited for adding new agents. The process would be:

1.  **Define State**: Create any new data structures needed for the agent's input in `src/agents/state.rs`.
2.  **Create Agent Module**: Create a new file, e.g., `src/agents/refactor_agent.rs`.
3.  **Implement the `Agent` Trait**: Define the new agent's struct and implement the `async fn run` method. This is where the agent's unique logic, prompt engineering, and tool design would reside.
4.  **Integrate with CLI**: Add a new subcommand in `src/cli.rs` and a corresponding `match` arm in `src/main.rs` to invoke the new agent.

The existing modules for API interaction (`gemini.rs`), logging (`logging.rs`), error handling (`error.rs`), and file utilities (`files.rs`) can be reused directly, making the process of adding new capabilities very efficient.
