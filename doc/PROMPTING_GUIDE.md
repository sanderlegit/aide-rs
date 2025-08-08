# Guide to Writing Effective Prompts for `aide-rs`

The `aide-rs` tool is designed to automate software development tasks by following a structured plan. The quality of this plan, and therefore the success of the agent, depends heavily on the quality of the initial prompt you provide. This guide explains how to write effective prompts to get the best results.

## The Philosophy: Plan, Verify, Execute

The core workflow of `aide-rs` is built on a simple but powerful idea:

1.  **Plan First**: A dedicated "architect" agent (`PlanAgent`) analyzes your goal and creates a detailed, step-by-step plan. It does not write any code.
2.  **Verify Rigorously**: The plan is broken down into small, verifiable tasks. Each task is validated by a set of commands you define.
3.  **Execute Iteratively**: An "implementer" agent (`ImplAgent`) executes the plan one task at a time, using the validation commands as its test suite to confirm success or self-correct on failure.

Your prompt is the primary input to this entire process. A good prompt is **clear, specific, and verifiable**.

## The `PlanPrompt` Structure

Your prompt must be a TOML file that defines the `PlanPrompt` structure. Let's break down each field.

### `objective`

This is the most important field. It's a high-level description of what you want to achieve.

-   **Be Specific**: Don't just say "build an app." Describe the app's features. What does it do? What are its inputs and outputs?
-   **Define Functionality**: List the specific functions, CLI subcommands, or API endpoints you want.
-   **Suggest Technologies**: If you have specific crates or technologies in mind (e.g., `axum`, `serde`, `lancedb`), mention them. This gives the agent a strong starting point.
-   **Use Multiline Strings**: For complex objectives, use TOML's `"""` multiline strings to provide detailed descriptions.

### `file_scoping`

This tells the agent which files it is allowed to read and modify. It's a critical safety and focus mechanism.

-   **`include`**: A list of glob patterns for files the agent should consider. Be as specific as possible. For a new feature, you might include `src/**/*.rs` and `Cargo.toml`.
-   **`exclude`**: A list of glob patterns to exclude from the scope. This is useful for protecting sensitive files or parts of the codebase you don't want the agent to touch.
-   **Principle of Least Privilege**: Only give the agent access to the files it absolutely needs to complete the objective.


### `coding_conventions`

This is where you enforce your project's standards. The agent is designed to follow these instructions strictly.

-   **Be Explicit**: Clearly state your rules for error handling (`thiserror`), logging (`tracing`), code style, and architecture.
-   **Provide Examples**: If you have a preferred pattern, describe it. For example: "Create a custom `Error` enum using `thiserror` and a `Result<T>` type alias."
-   **Set Expectations**: Mention requirements like doc comments for public APIs.

### `formatter_command`

An optional command that the `ImplAgent` will run after it modifies files for a task. This ensures all generated code conforms to your project's formatting standards.

-   **Example**: `formatter_command = "cargo fmt --all"`

### `validation_commands`

This is the agent's "test suite." It's a list of shell commands that are run after the formatter for each task. A task is only considered successful if **all** validation commands pass with their `expected_exit_code`.

-   **Be Thorough**: Include commands that check for correctness, style, and compilability.
-   **Good Examples**:
    -   `{ command = "cargo check", expected_exit_code = 0 }`
    -   `{ command = "cargo clippy -- -D warnings", expected_exit_code = 0 }`
    -   `{ command = "cargo test", expected_exit_code = 0 }`
    -   `{ command = "cargo build", expected_exit_code = 0 }`
-   **Think Incrementally**: These commands run for *each task*. This ensures that the project is in a working state at every step of the implementation.

---

## A Complete Example: The LanceDB Audio Manager

Here is a comprehensive example prompt that uses all the principles described above to create a new Rust CLI application from scratch.

**File: `prompts/lancedb_example.toml`**

```toml
objective = """
Create a command-line tool in Rust named `audio_db` that uses LanceDB to manage a collection of `.wav` audio files.

The tool should have the following subcommands:
1. `add <PATH>`: Reads a `.wav` file from the given path, extracts its properties (channels, sample rate, duration), and adds a record to a LanceDB table. The table should be created if it doesn't exist. The record should store the file path, channels, sample rate, and duration in seconds.
2. `list`: Queries the LanceDB table and prints a formatted list of all audio files, including their ID, path, and properties.
3. `play <ID>`: Plays the audio file corresponding to the given ID from the database.

Key Crates to Use:
- `lancedb`: For database operations.
- `clap`: For parsing command-line arguments.
- `hound`: For reading `.wav` file headers and data.
- `rodio`: For audio playback.
- `thiserror`: For custom error handling.
- `tokio`: For the async runtime.
- `tracing`: For logging.
"""


# Define the files the agent should look at for context and modify.
# For a new project, we start with just the core files.
[file_scoping]
include = ["src/**/*.rs", "Cargo.toml"]
exclude = []

# Provide detailed coding conventions for the agent to follow.
coding_conventions = """
- The project must be a binary crate.
- All dependencies must be added to `Cargo.toml`.
- Use `clap` with the `derive` feature for CLI parsing. Define a `Cli` struct and a `Commands` enum for the subcommands.
- Create a custom `Error` enum using `thiserror` to handle all potential failures (I/O, LanceDB, audio processing, etc.).
- All public functions, structs, and enums must have clear doc comments.
- Use the `tracing` crate for logging. Set up a simple subscriber in `main.rs`.
- The LanceDB table should be stored at a default path like `./data/audio.lance`. The `add` command should create this directory if it doesn't exist.
- For the `play` command, read the audio data from the file path stored in LanceDB and use `rodio` to play it back.
- The code should be well-structured. Consider creating separate modules for CLI, database logic, and audio handling if the complexity grows.
"""

# The command to run to format the code. This ensures consistency.
formatter_command = "cargo fmt --all"

# A sequence of commands that must pass for each task to be considered complete.
# This acts as a CI/CD pipeline for the agent.
[[validation_commands]]
command = "cargo check"
expected_exit_code = 0

[[validation_commands]]
command = "cargo clippy -- -D warnings"
expected_exit_code = 0

[[validation_commands]]
command = "cargo test"
expected_exit_code = 0

[[validation_commands]]
command = "cargo build"
expected_exit_code = 0
```

### Analysis of the Example

-   The **objective** is highly detailed, specifying the exact subcommands and even suggesting the necessary crates. This removes ambiguity.
-   The **file scope** is minimal, which is appropriate for a new project.
-   The **coding conventions** are prescriptive, ensuring the agent produces code that follows best practices for error handling, CLI design, and documentation.
-   The **validation commands** create a robust check for each task, ensuring that the project remains compilable and free of warnings at all times.

By providing such a detailed prompt, you empower the `PlanAgent` to create a high-quality, granular plan, which in turn enables the `ImplAgent` to execute it successfully.
