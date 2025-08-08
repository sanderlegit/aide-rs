# Phase 2: CLI Definition and State Management

**Status: Complete**

## Goal
Define the user-facing command-line interface and the core data structures that will represent the application's state. This phase connects the user's commands to the internal data models.

## Tasks
1.  **Define CLI Structure**:
    *   In `src/cli.rs`, use `clap` with its `derive` feature to implement the `Cli` struct.
    *   Define the `plan` and `impl` subcommands with their respective arguments (`--prompt`, `--plan`, `--auto-commit`, etc.) as specified in `spec/00_implementation_plan.md`.
2.  **Define State Structs**:
    *   In `src/agents/state.rs`, implement all data models required for the workflow.
    *   These include `PlanPrompt`, `FileScope`, `ImplementationPlan`, `Task`, `TaskStatus`, and `ValidationStep`. The `Task` struct will contain a description and validation steps, but not its own file scope, as scoping is handled globally for the entire plan.
    *   Derive `serde::Serialize` and `serde::Deserialize` for all models to allow for serialization to/from JSON and TOML.
3.  **Integrate CLI in `main.rs`**:
    *   Update `main.rs` to parse the command-line arguments using `Cli::parse()`.
    *   Add a `match` statement to dispatch control based on the parsed subcommand (`Commands::Plan` or `Commands::Impl`), calling placeholder functions for now.

## Test Coverage
*   **Unit Tests**:
    *   Add unit tests for `agents/state.rs` to confirm that each data structure can be successfully serialized to and deserialized from a representative JSON or TOML string. This verifies the `serde` implementation.
*   **E2E Tests (Scaffolding)**: While full E2E tests come later, running the compiled binary with `--help` will serve as a basic, manual validation of the `clap` setup.

## Completion Criteria
*   Running `cargo run -- --help` displays the full help message, including details for the `plan` and `impl` subcommands.
*   The state structs in `agents/state.rs` pass their serialization/deserialization tests.
*   The project compiles cleanly.
