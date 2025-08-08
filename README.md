# `aide-rs`

An AI-powered software development agent that automates coding tasks by generating and executing implementation plans.

## Overview

`aide-rs` is a command-line tool that uses Google's Gemini models to understand a software development objective, create a step-by-step plan, and then implement that plan by editing files. It operates in a two-phase process:

1.  **Plan Phase (`aide-rs plan`)**: An expert "architect" agent analyzes your objective, the existing code, and your coding conventions to produce a structured TOML implementation plan.
2.  **Implement Phase (`aide-rs impl`)**: An expert "pair programmer" agent executes the plan task by task. It edits code, runs validation commands (like tests or linters), and can even summarize compiler errors to self-correct on failure.

The agent relies heavily on Gemini's **Function Calling** capabilities to ensure structured and reliable operations, rather than parsing unpredictable text.

## Features

-   **Two-Agent System**: A `PlanAgent` for high-level strategy and an `ImplAgent` for tactical execution.
-   **Dependency Research**: Can use Google Search to find the best and most up-to-date libraries for a given objective.
-   **Structured Interaction**: Uses Gemini Function Calling for all file modifications, ensuring robustness.
-   **Self-Correction**: Can analyze validation failures, summarize errors, and retry tasks.
-   **Git Integration**: Automatically commits the work upon successful completion of all tasks.
-   **Scoped Context**: Uses glob patterns and `.gitignore` to provide the agent with only the relevant file context.

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

3.  **Build the project:**
    ```bash
    cargo build --release
    ```
    The compiled binary will be available at `target/release/aide-rs`. You can copy this to a location in your `PATH` for easier access.

## Usage

`aide-rs` is designed to be run from the root of the project you want to modify. The project should be a Git repository, especially if you plan to use the `--auto-commit` feature.

### Step 1: Create a Prompt File

First, define your objective in a TOML file. This file tells the agent what you want to achieve, which files it should consider, and how to validate its work. For complex objectives or detailed coding conventions, you can use TOML's multiline strings (`"""..."""`).

Create a file, for example, `prompts/add_feature.toml`:

```toml
# prompts/add_feature.toml

objective = """
Add a new function `hello_world()` to `src/lib.rs` that prints 'Hello, World!' to the console.
Suggest and use a popular crate for colored terminal output to make the message stand out.
"""

# Use Google Search to find a suitable library for the objective.
use_google_search_for_deps = true

# Define the files the agent should look at for context.
# It uses glob patterns and respects .gitignore.
[file_scoping]
include = ["src/**/*.rs", "Cargo.toml"]
exclude = []

# Provide coding conventions for the agent to follow.
coding_conventions = """
All functions must have a doc comment.
Follow standard Rust formatting (`cargo fmt`).
"""

# Define commands that must pass for a task to be considered complete.
[[validation_commands]]
command = "cargo check"
expected_exit_code = 0
```

### Step 2: Generate the Implementation Plan

Run the `plan` command to have the AI architect create a plan.

```bash
# Make sure you are in the root of your target git repository
/path/to/aide-rs plan --prompt prompts/add_feature.toml
```

This will generate a uniquely named plan file in the `.ai/` directory, such as `.ai/plan_add_feature_1678886400.toml`. This file contains the sequence of tasks the agent will perform. You can review this file before proceeding.

### Step 3: Execute the Plan

Now, run the `impl` command to have the AI programmer execute the plan. You can pass the path to the generated plan file.

```bash
# This will execute the plan, edit files, and run validation.
/path/to/aide-rs impl --plan .ai/plan_add_feature_1678886400.toml
```

If all tasks are completed successfully, the changes will be applied to your files.

#### Auto-Committing

To have the agent automatically create a Git commit after each task succeeds, use the `--auto-commit` flag.

```bash
/path/to/aide-rs impl --plan .ai/plan_add_feature_1678886400.toml --auto-commit
```
This will create a new commit for each successfully completed task, with a message derived from the task's description.

#### Enriching Errors with Google Search

If the agent gets stuck on a complex error (e.g., a tricky compiler issue or a dependency conflict), you can use the `--enrich-errors` flag. This instructs the agent to use Google Search to find relevant documentation, examples, or solutions for the error it encountered, and then add that research to its context for the next attempt.

```bash
/path/to/aide-rs impl --plan .ai/plan_add_feature_1678886400.toml --enrich-errors
```

## Development

To run the test suite, which includes unit, integration, and end-to-end tests:

```bash
cargo test
```
Note: Tests run sequentially (`--test-threads=1`) to avoid race conditions with environment variables.
