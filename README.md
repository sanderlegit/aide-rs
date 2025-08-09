# `aide-rs`

An AI-powered software development agent that automates coding tasks by generating and executing implementation plans.

## Overview

`aide-rs` is a command-line tool that uses Google's Gemini models to understand a software development objective, create a step-by-step plan, and then implement that plan by editing files. It operates using a flexible, flow-based architecture.

A "Flow" is a declarative YAML file that defines a sequence of prompt-driven steps, allowing for complex workflows like planning, coding, and self-correction to be easily defined and customized.

## Features

-   **Declarative Workflows**: Define agent behavior in simple YAML files instead of hardcoding it.
-   **Self-Correction**: Flows can define verification steps (like running `cargo check`) and retry logic on failure.
-   **Structured Tool Use**: Relies on Gemini's Function Calling capabilities for reliable operations like file modifications and task generation.
-   **Scoped Context**: Uses a system of mergeable YAML files (`ctx/*.yml`) and `.gitignore` to provide the agent with precise file context.
-   **Extensible**: Easily create new flows and tools to automate any development task.

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

`aide-rs` is designed to be run from the root of the project you want to modify. It operates by executing "Flows"—declarative YAML files that define a sequence of prompt-driven steps.

### Step 1: Define Your Objective

First, create a simple YAML file that describes your high-level goal and the files the agent should be aware of.

**`my_feature.yml`:**
```yaml
objective: |
  Add a new function `hello_world()` to `src/lib.rs` that prints 'Hello, World!' to the console.
  Then, call this new function from `main.rs`.

# Define the files the agent should look at for context.
# It uses glob patterns and respects .gitignore.
file_scoping:
  include:
    - "src/**/*.rs"
    - "Cargo.toml"
  exclude: []

# Provide coding conventions for the agent to follow.
coding_conventions: |
  All public functions must have a doc comment.
  Follow standard Rust formatting (`cargo fmt`).
```

### Step 2: Run a Flow

`aide-rs` comes with pre-defined flows for common activities like planning and coding. You can see all available flows by running `aide-rs list`.

To execute a flow, use the `run` command, specifying the flow name and your prompt file.

#### Example 1: Generating a Plan

The `plan` flow analyzes your objective and creates a structured list of tasks, but does not execute them.

```bash
# Make sure you are in the root of your target git repository
aide-rs run plan --prompt my_feature.yml
```
This will output the plan to your console and save it to the `.ai/` directory.

#### Example 2: Implementing Code

The `code` flow will analyze the objective, create a plan, and then immediately attempt to implement it, running validation commands along the way.

```bash
# This will execute the plan, edit files, and run validation.
aide-rs run code --prompt my_feature.yml
```

If all tasks are completed successfully, the changes will be applied to your files.

### Creating Your Own Flows

The true power of `aide-rs` comes from creating your own `*.yml` files in the `flows/` directory. You can define custom sequences of prompts, tools, and validation logic to automate any development task. For a complete guide, see the [Architecture Documentation](doc/refactor_architecture.md).

## Development

To run the test suite, which includes unit, integration, and end-to-end tests:

```bash
cargo test
```
Note: Tests run sequentially (`--test-threads=1`) to avoid race conditions with environment variables.
