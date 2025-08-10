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

-   [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
-   A Google Gemini API Key. See [Google AI for Developers](https://ai.google.dev/) to get one.

## Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/your-username/aide-rs.git
    cd aide-rs
    ```
    *(Note: Replace `your-username/aide-rs.git` with the actual repository URL)*

2.  **Set up your API Key:**
    Create a `.env` file in the root of the project and add your Gemini API key:
    ```
    GEMINI_API_KEY=your_api_key_here
    ```
    The application will load this variable automatically.

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

### Workflows

`aide-rs` supports several workflows, from interactive, step-by-step development to fully automated execution.

#### Interactive Workflow: `research`, `plan`, and `implement`

For complex tasks, you can use the commands sequentially. This gives you full control at each stage.

1.  **Research (Optional)**: If you're unsure about the best libraries or patterns to use, start with a research step. `aide-rs` will use a search-enabled LLM to gather information.
    ```bash
    aide-rs research "best rust crates for audio processing" src/main.rs
    ```
    This produces a `research.md` file and launches `aider` for you to review and refine it.

2.  **Plan**: Once you have a clear goal, use the `plan` command. This uses an LLM to break down your objective into a markdown task list.
    ```bash
    aide-rs plan "Create a command-line tool to manage audio files using lancedb" src/main.rs Cargo.toml
    ```
    This creates a `plan.md` and launches `aider` for refinement.

3.  **Implement**: After planning, use the `implement` command to start coding. This command launches `aider` with your objective and files, ready for you to start pair-programming with the AI.
    ```bash
    aide-rs implement "Implement the 'add' subcommand for the audio tool" src/main.rs src/db.rs
    ```

#### Automated Implementation: `implement --auto`

For smaller, well-defined tasks like fixing a bug, you can use the automated implementation loop.

```bash
aide-rs implement "Fix the compilation errors" src/main.rs src/db.rs --auto --validate-cmd "cargo check"
```

This loop will:
1.  Run `aider` to attempt a fix.
2.  Run `cargo check`.
3.  If it fails, `aide-rs` uses an LLM to analyze the error, looks up relevant documentation with its `doc_retriever` tool, and feeds the context back to `aider` for the next attempt.
4.  If it succeeds, the loop finishes and the changes are committed.

### Configuration

#### File Filtering

`aide-rs` automatically filters the files included in the context using rules defined in `.ai/filter=all`. This file uses glob patterns to include or exclude files and directories. You can customize it to suit your project's needs.

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
When you provide file paths to commands like `plan` or `implement` (e.g., `aide-rs plan "..." .`), `aide-rs` will walk the directories and apply these filters to determine the final list of files used.

### Fully Automated Workflow: The `run` Command

The `run` command orchestrates a complete, non-interactive workflow from a single configuration file. This is ideal for CI/CD pipelines or complex, automated refactoring tasks.

First, create a YAML file (e.g., `feature.yml`):
```yaml
# feature.yml
objective: "Add a new 'list' subcommand to the audio tool to display all entries from the database."
files:
  - src/main.rs
  - src/db.rs
validate_cmd: "cargo check"
```

Then, execute it:
```bash
aide-rs run feature.yml
```

This single command will:
1.  Start a new session.
2.  Run the **plan** strategy to generate a task list (`plan.md`).
3.  Run the **implement** strategy in fully automated mode, using the generated plan as the objective.
4.  Use the automated debugging loop (`--auto`) to fix issues until `validate_cmd` succeeds.
5.  Commit the final changes.

## Development

To run the test suite, which includes unit, integration, and end-to-end tests:

```bash
cargo test
```
Note: Tests run sequentially (`--test-threads=1`) to avoid race conditions with environment variables.
