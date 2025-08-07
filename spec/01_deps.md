### Main Dependencies (`[dependencies]`)

These packages are essential for the core functionality of your AI agent.

```toml
[dependencies]
# Core asynchronous runtime
tokio = { version = "1.38.0", features = ["full"] }

# Command-line interface parsing
clap = { version = "4.5.4", features = ["derive"] }

# Serialization and deserialization
# The `derive` feature is crucial for easily making your structs serializable.
serde = { version = "1.0.203", features = ["derive"] }
serde_json = "1.0.117" # For handling JSON data. [32]
toml = "0.8.13" # For parsing the TOML prompt files. [17]

# Error handling
# A helper for creating custom error types. [2, 5, 6, 9]
thiserror = "1.0.61"

# Logging and diagnostics
# The core tracing API for instrumenting your application. [1, 10, 30]
tracing = "0.1.40"
# Subscriber for processing and displaying trace data. [12, 16]
tracing-subscriber = { version = "0.3.18", features = ["env-filter"] }

# File system interactions
# For recursive directory walking and respecting .gitignore files. [15, 23]
ignore = "0.4.22"
# For glob pattern matching, used by the 'ignore' crate.
glob = "0.3.1"

# Version Control System (Git)
# Provides bindings to libgit2 for Git operations.
git2 = "0.18.3"

# Google Gemini API Client
# A client for interacting with the Google Gemini API. [4]
gemini-client-rs = "0.1.0"
```

### Development Dependencies (`[dev-dependencies]`)

These packages are used for testing your application and are not included in the final production binary.

```toml
[dev-dependencies]
# HTTP mocking for testing API interactions deterministically. [7, 20, 33]
wiremock = "0.6.0"

# Assertion library for command-line application testing.
assert_cmd = "2.0.14"

# For creating temporary files and directories in tests. [18, 21, 37]
tempfile = "3.10.1"
```

### Explanation of Key Crates:

*   **`tokio`**: An asynchronous runtime essential for handling concurrent operations, especially network requests to the Gemini API and running validation commands.
*   **`clap`**: A powerful and widely-used library for parsing command-line arguments, making it easy to define your `plan` and `impl` subcommands.
*   **`serde`**, **`serde_json`**, **`toml`**: The standard ecosystem for serialization and deserialization in Rust. You'll use `serde`'s `derive` macros to make your `PlanPrompt`, `ImplementationPlan`, and `Task` structs easily convertible to and from JSON and TOML formats.
*   **`thiserror`**: Greatly simplifies error handling by allowing you to create clean, descriptive error enums without boilerplate code.
*   **`tracing`** & **`tracing-subscriber`**: A modern framework for structured logging. It's more powerful than the standard `log` crate, especially for async applications, as it can trace the entire lifecycle of a task.
*   **`ignore`**: The perfect tool for implementing your `FileScope` logic. It respects `.gitignore` rules by default and provides a fast, parallel directory walker.
*   **`git2`**: The standard library for programmatic Git operations in Rust, necessary for the `--auto-commit` feature.
*   **`gemini-client-rs`**: A client library specifically for the Google Gemini API, which is central to your agent's functionality.
*   **`wiremock`**: Critical for integration testing. It allows you to create a mock HTTP server that can simulate the Gemini API, enabling you to test your agent's logic without making actual API calls, which is faster, cheaper, and more predictable.
*   **`assert_cmd`** & **`tempfile`**: The go-to combination for end-to-end testing of CLI applications. `assert_cmd` lets you run your compiled binary and make assertions about its output and exit code, while `tempfile` provides a safe way to create temporary project structures and files for your tests to run against.
