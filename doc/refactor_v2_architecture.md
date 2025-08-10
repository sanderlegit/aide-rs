# Architecture Refactor v2: Aider-Centric Workflow Engine

This document outlines a comprehensive architectural refactoring for `aide-rs`. The goal is to move from the custom, flow-based system to a more robust and powerful model that uses the external `aider` tool as its primary implementation engine.

## 1. Core Philosophy

The new architecture is built on the idea of `aide-rs` acting as a **meta-agent** or **orchestrator**. Instead of defining agent logic in YAML "Flows", `aide-rs` will manage and execute a series of "Strategies" (e.g., Research, Plan, Implement) that prepare context and then delegate the core task of code modification to `aider`.

The core pillars of this new philosophy are:
1.  **Delegate to the Specialist**: Use `aider` for what it's best at: interactive, git-aware, file-based code editing.
2.  **Orchestrate and Enhance**: Use `aide-rs` to provide value *around* `aider`—by enriching prompts with research, providing debugging context from compile errors, and managing complex, multi-step workflows.
3.  **Session-Based and Interactive**: Each run is a self-contained "session" with its own history and logs. The system is designed for both fully automated execution and interactive use.

## 2. Core Components

The refactor will introduce several new or heavily modified components:

-   **Orchestrator (`src/orchestrator.rs`)**: The new heart of the application, replacing `FlowRunner`. It manages sessions, executes strategies, and controls the main application loop (both interactive and automated).

-   **Aider Wrapper (`src/agents/aider.rs`)**: A new module responsible for building and executing `aider` commands. It will manage `aider`'s environment, including:
    -   Constructing command-line arguments (`--chat-history-file`, `--test-cmd`, etc.).
    -   Creating and passing initial prompts via `--message-file`.
    -   Running `aider` as a subprocess and capturing its output and exit codes.

-   **Gemini Wrapper (`src/agents/gemini.rs`)**: An evolution of the existing `GeminiClientWrapper`. It will be enhanced to support distinct modes of operation:
    -   **Research Mode**: Enables grounding via Google Search to find up-to-date information (e.g., latest crate versions, design patterns).
    -   **Debug Mode**: Takes a compiler error and code context as input and suggests which documentation to retrieve with `doc_retriever`.

-   **Session Manager (`src/session.rs`)**: A new utility for creating and managing session-specific directories. This ensures that all artifacts from a run (logs, history, research notes) are isolated and organized.

## 3. Command-Line Interface (CLI) Refactor

The CLI will be updated to reflect the new, strategy-based approach.

```
aide-rs <COMMAND>

COMMANDS:
    research      Launch a research session to investigate a topic and produce documentation.
        <OBJECTIVE>             The research topic (e.g., "best rust crates for audio processing").

    plan          Launch a planning session to break an objective into a task list.
        <OBJECTIVE>             The high-level goal to be planned.

    implement     Launch an implementation session to work on code.
        <OBJECTIVE>             The task to implement.
        --validate-cmd <CMD>    The command to run to validate changes (default: "make test").
        --auto                  Run in a fully automated loop, attempting to fix errors until validation passes.

    run           Execute a non-interactive, multi-stage workflow from a config file.
        <PROMPT_FILE.YML>       A YAML file defining the objective and configuration for the run.
```

## 4. Execution Strategies

These strategies define the high-level workflows the `Orchestrator` can execute.

### Research Strategy
1.  **Initiate**: User runs `aide-rs research "my topic"`.
2.  **Session Start**: `SessionManager` creates a new session directory (e.g., `.ai/sessions/20250809_103000/`).
3.  **Query Gemini**: `Orchestrator` calls `GeminiWrapper` in "Research Mode" with the objective.
4.  **Store Artifact**: The Markdown response is saved to `<session_dir>/research.md`.
5.  **Delegate to Aider**: `Orchestrator` can optionally launch `aider` with `research.md` added to the chat, allowing the user to interactively refine the research document.

### Plan Strategy
1.  **Initiate**: User runs `aide-rs plan "my objective"`.
2.  **Session Start**: A new session is created.
3.  **Context Gathering**: The `Orchestrator` can optionally run the **Research Strategy** first to provide context.
4.  **Query Gemini**: `Orchestrator` calls `GeminiWrapper` with the objective and a "planning" system prompt.
5.  **Delegate to Aider**: The response (a markdown task list) is passed as the initial message to a new `aider` session. The user can then interactively refine the plan with `aider`. The final plan is saved to `<session_dir>/plan.md`.

### Implement Strategy (Automated Loop)
This is the core automated workflow, which orchestrates `aider`'s test-driven development capabilities.
1.  **Initiate**: User runs `aide-rs implement "my task" --auto --validate-cmd "cargo check"`.
2.  **Session Start**: A new session is created. The `Orchestrator` enters a retry loop.
3.  **Delegate to Aider**: `Orchestrator` calls `AiderWrapper`, providing the task.
4.  **Aider Commits**: `aider` applies code changes and commits them.
5.  **Validate**: `aide-rs` runs the validation command itself.
6.  **Check Result**:
    -   **On Success**: The loop terminates.
    -   **On Failure**: `aide-rs` captures the `stdout` and `stderr` from the failed validation. The original commit is not reverted.
        a. The test failure output is captured.
        b. `Orchestrator` calls `GeminiWrapper` in "Debug Mode" with the error, asking it to identify relevant APIs or concepts to look up.
        c. The Gemini response is used to invoke our `doc_retriever` tool.
        d. The retrieved documentation is saved to `<session_dir>/debug_context.md`.
        e. The `Orchestrator` re-invokes `aider`, adding the `debug_context.md` to the chat and a new prompt: "The previous attempt failed with this error. Here is some context from the documentation. Please fix the issue."
        f. The loop continues until success or `max_retries` is reached.

## 5. Directory and Logging Structure

A clean, session-based structure is essential for traceability.

```
.ai/
└── sessions/
    └── <session_id>/              (e.g., 20250809_103000_plan_add_new_feature)
        ├── orchestrator.log       # High-level log from the aide-rs orchestrator.
        ├── aider.chat.history.md  # The chat history file for this session's aider instance.
        ├── aider.input.history    # The input history file.
        ├── research.md            # Artifact from a research step.
        ├── plan.md                # Artifact from a planning step.
        ├── debug_context.md       # Docs retrieved during a debug loop.
        └── gemini_logs/
            ├── 01_research_request.json
            ├── 01_research_response.json
            └── ...
```

## 6. Migration Plan

This is a significant refactor. The following steps outline a path to implementation:

1.  **Task 1: Build Core Components.**
    -   Create the `SessionManager` to handle directory creation.
    -   Implement the `AiderWrapper` to reliably call the `aider` CLI with the necessary arguments for isolated sessions.
    -   Enhance the `GeminiWrapper` to support the distinct "Research" and "Debug" modes.

2.  **Task 2: Refactor the CLI.**
    -   Update `src/cli.rs` and `src/main.rs` to reflect the new command structure (`research`, `plan`, `implement`).
    -   Remove the old `FlowRunner`-based logic.

3.  **Task 3: Implement the Orchestrator.**
    -   Build the main application logic in `src/orchestrator.rs`.
    -   Implement the state machine and logic for the **Implement Strategy** automated loop.

4.  **Task 4: Deprecate and Remove Old Code.**
    -   Delete `src/runner.rs`, `src/flows/`, and `src/prompt.rs`.
    -   Remove associated tests.
    -   Update `README.md` and other documentation to reflect the new UX.

5.  **Task 5: Add New E2E Tests.**
    -   Create new tests in `tests/e2e.rs` that validate the new strategies, mocking `aider` and `gemini` calls where necessary.
