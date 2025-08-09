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

### Example Workflow

Here's how you might use `aide-rs` to add a new feature to your project.

#### 1. Research (Optional)

If you're unsure about the best libraries or patterns to use, you can start with a research step. `aide-rs` will use a search-enabled LLM to gather information.

```bash
aide-rs research "best rust crates for audio processing" src/main.rs
```
This will produce a research document in a new session directory (e.g., `.ai/sessions/.../research.md`).

#### 2. Plan

Once you have a clear goal, use the `plan` command. This will use an LLM to break down your objective into a markdown task list, which is then passed to `aider` for you to review and refine.

```bash
aide-rs plan "Create a command-line tool to manage audio files using lancedb" src/main.rs Cargo.toml
```

#### 3. Implement

After planning, use the `implement` command to start coding. This command launches `aider` with your objective and files, ready for you to start pair-programming with the AI.

```bash
aide-rs implement "Implement the 'add' subcommand for the audio tool" src/main.rs src/db.rs
```

For automated workflows, you can use the `--auto` flag, which will run a validation command in a loop until it succeeds.

```bash
aide-rs implement "Fix the compilation errors" src/main.rs src/db.rs --auto --validate-cmd "cargo check"
```

This loop will:
1. Run `aider` to attempt a fix.
2. Run `cargo check`.
3. If it fails, feed the error back into the context for the next attempt.
4. If it succeeds, the loop finishes.

## Development

To run the test suite, which includes unit, integration, and end-to-end tests:

```bash
cargo test
```
Note: Tests run sequentially (`--test-threads=1`) to avoid race conditions with environment variables.
