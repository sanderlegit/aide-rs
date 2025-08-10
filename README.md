# `aide-rs`

An AI-powered software development agent that automates coding tasks by generating and executing implementation plans.

## Overview

`aide-rs` is an AI-powered software development agent that acts as a smart orchestrator for `aider`. It uses large language models to manage and execute high-level strategies (e.g., Research, Plan, Implement), preparing context and then delegating the core task of code modification to `aider`.

Instead of a simple prompt-and-response loop, `aide-rs` manages complex, multi-step workflows, providing `aider` with rich context from research, documentation, and even compilation errors.

## Features

-   **Strategy-Based Workflows**: Executes high-level strategies like `research`, `plan`, and `implement` to accomplish complex tasks.
-   **Aider-Centric**: Delegates all code editing to `aider`, leveraging its powerful features for interactive, git-aware development.
-   **Automated Debugging Loop**: In automated mode, `aide-rs` can run a validation command (e.g., `make test`), analyze failures, use an LLM to look up relevant documentation, and re-run `aider` with the new context to fix the issue.
-   **Session-Based Artifacts**: Each run is isolated in its own session directory, containing all logs, research notes, and `aider` history for full traceability.

## Prerequisites

-   [Rust](https://www.rust-lang.org/tools/install) (latest stable version).
-   [Aider](https://github.com/paul-gauthier/aider), installed and available in your `PATH`.
-   A Google Gemini API Key. See [Google AI for Developers](https://ai.google.dev/) to get one.

## Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/your-username/aide-rs.git
    cd aide-rs
    ```
    *(Note: Replace `your-username/aide-rs.git` with the actual repository URL)*

2.  **Configure your environment:**
    Create a `.env` file in the root of the project to store your API key and configure the `aider` command.
    ```
    AIDER_COMMAND="aider --yes --dark-mode --model vertex_ai/gemini-2.5-pro"
    GEMINI_API_KEY=your_api_key_here
    ```
    -   `AIDER_COMMAND`: The exact `aider` command to run. You can customize this with your preferred `aider` settings. `aide-rs` will append its own arguments to this command.
    -   `GEMINI_API_KEY`: Your key for the Google Gemini API.

    The application will load these variables automatically.

3.  **Build and install the project:**
    ```bash
    make install
    ```
    This will build the project in release mode and copy the `aide-rs` and `doc-retriever` binaries to `~/.local/bin/`. Make sure this directory is in your `PATH`.

    Alternatively, you can build manually:
    ```bash
    cargo build --release
    ```
    The compiled binaries will be available at `target/release/aide-rs` and `target/release/doc-retriever`. You should copy both to a location in your `PATH`.

## Usage

`aide-rs` is designed to be run from the root of the project you want to modify. It acts as an orchestrator for the `aider` tool, enriching its context with research and structured planning.

You can specify the Gemini model to use with the global `--model` flag (e.g., `--model gemini-1.5-flash`). This will apply to all commands.

### Workflows

`aide-rs` supports several workflows, from interactive, step-by-step development to fully automated execution.

#### Interactive Workflow: `research`, `plan`, and `implement`

For complex tasks, you can use the commands sequentially. This gives you full control at each stage.

1.  **Research (Optional)**: If you're unsure about the best libraries or patterns to use, start with a research step. `aide-rs` will use a search-enabled LLM to gather information.
    ```bash
    aide-rs research --context all "best rust crates for audio processing"
    ```
    This produces a `research.md` file and launches `aider` for you to review and refine it.

2.  **Plan**: Once you have a clear goal, use the `plan` command. This uses an LLM to break down your objective into a markdown task list.
    ```bash
    aide-rs plan --context all "Create a command-line tool to manage audio files using lancedb"
    ```
    This creates a `plan.md` and launches `aider` for refinement.

3.  **Implement**: After planning, use the `implement` command to start coding. This command launches `aider` with your objective and files, ready for you to start pair-programming with the AI.
    ```bash
    aide-rs implement --context all "Implement the 'add' subcommand for the audio tool"
    ```

#### Automated Implementation: `implement --auto`

For smaller, well-defined tasks like fixing a bug, you can use the automated implementation loop.

```bash
aide-rs implement --context all "Fix the compilation errors" --auto --validate-cmd "cargo check" --allow-shell-commands
```

This loop works by orchestrating `aider` with a validation command. Here's the process:
1.  `aide-rs` invokes `aider` with the objective.
2.  `aider` attempts to fix the code and commits its changes.
3.  After `aider` finishes, `aide-rs` runs the validation command (e.g., `cargo check`).
4.  **If the validation command succeeds**, `aide-rs` considers the step successful and the loop finishes.
5.  **If the validation command fails**, `aide-rs` reverts `aider`'s last commit, uses an LLM to analyze the error output, looks up relevant documentation with its `doc_retriever` tool, and then re-runs `aider` with the new context to try again.

The `--allow-shell-commands` flag is recommended for automated runs to let the agent execute commands if needed, but can be omitted for safety.

#### Fully Automated Workflow: The `run` Command

The `run` command orchestrates a complete, non-interactive workflow from a single configuration file. This is ideal for CI/CD pipelines or complex, automated refactoring tasks.

For a detailed breakdown of how a workflow file is executed, see the [annotated example workflow](doc/annotated_workflow.yml).

First, create a YAML file (e.g., `feature.yml`) that defines a sequence of steps:
```yaml
# feature.yml
steps:
  - type: plan
    objective: "Add a new 'list' subcommand to the audio tool to display all entries from the database."
    context: "all"
    model: "gemini-1.5-flash" # Optional: specify a model for this step
  - type: implement
    objective: "Implement the plan."
    context: "all"
    validate_cmd: "cargo check"
```

Then, execute it:
```bash
aide-rs run feature.yml
```

This single command will execute the steps in order:
1.  Start a new session.
2.  Run the **plan** step to generate a task list (`plan.md`).
3.  Run the **implement** step in fully automated mode. It will automatically use the `plan.md` from the previous step as its primary objective.
4.  Use the automated debugging loop to fix issues until `validate_cmd` succeeds.
5.  Commit the final changes.

## Tools

`aide-rs` includes helper tools that can also be used standalone.

### `doc-retriever`

The `doc-retriever` tool allows you to fetch Rust documentation for crates and specific items (structs, enums, functions, etc.) and view it as structured JSON. This is the same tool `aide-rs` uses internally to provide documentation context to the LLM during the automated debugging loop.

It must be run from within a Rust project's directory.

**Usage:**

-   **Get crate-level documentation:**
    ```bash
    doc-retriever crate --name your_crate
    ```
    This will output JSON containing the crate's root documentation, version, and list of top-level modules.

-   **Get item-specific documentation:**
    ```bash
    doc-retriever item --crate your_crate --path "your_crate::module::MyStruct"
    ```
    This will output JSON with detailed documentation for `MyStruct`, including its methods and trait implementations.

## Configuration

### File Filtering

`aide-rs` filters the files provided to the context based on rules in a filter file. By default, it looks for `.ai/filter=all`, but you can specify a different context (e.g., `backend`) with the `--context` flag, which will cause it to look for `.ai/filter=backend`.

This file uses glob patterns to include or exclude files and directories. You can customize it to suit your project's needs.

The filter file is composed of sections, starting with `#include` or `#exclude`.

**Example `.ai/filter=all`:**
```
#exclude
.git
.ai
target/
*.lock

#include
*.rs
*.toml
*.md
Makefile
```
When you provide file paths to commands like `plan` or `implement`, or use the `--context` flag, `aide-rs` will walk the directories and apply these filters to determine the final list of files used.

## Future Work

With the core architecture in place, future work will focus on enhancing the agent's capabilities and improving user experience. Key areas for development include:

-   **Expanded Toolset**: Implement additional tools beyond `doc_retriever`, such as a `file_system` tool for creating, reading, and listing files, which would enhance the agent's ability to interact with the project structure.
-   **Improved Interactive Experience**: Enhance the interactive modes (`plan`, `research`, `implement`) with better user feedback and more control over the agent's actions.
-   **Configuration Flexibility**: Allow more granular configuration of strategies within the `run.yml` file, such as specifying which tools are enabled for a given run.

## Development

To run the test suite, which includes unit, integration, and end-to-end tests:

```bash
make test
```
Alternatively, you can run the tests directly with Cargo. It is recommended to run them sequentially to avoid race conditions in tests that rely on shared resources like environment variables or mock servers.
```bash
cargo test -- --test-threads=1
```
