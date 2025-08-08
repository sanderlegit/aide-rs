# Development, Testing, and Robustness

This document outlines the development methodology, testing strategy, and robustness mechanisms of the `aide-rs` project.

## Development Methodology

The project was developed in a structured, phased approach, with each phase having a clear goal, a set of tasks, and defined completion criteria. This methodology ensures that each component is built and tested systematically before being integrated into the larger system. The phases included:

1.  **Project Setup**: Establishing the foundational structure, dependencies, error handling, and logging.
2.  **CLI and State**: Defining the user-facing interface and the core data structures for managing state.
3.  **Core Logic**: Implementing standalone modules for file system and version control interactions.
4.  **Agent Implementation**: Building the core `PlanAgent` and `ImplAgent` and integrating with the Gemini API.
5.  **Testing**: Implementing a comprehensive testing suite, including end-to-end tests.
6.  **Log Summarization**: Adding features to improve robustness when handling large error outputs.

This phased approach promotes modularity and allows for incremental development and testing.

## Modularity and Maintainability

The codebase is designed to be modular and maintainable, following standard Rust conventions and best practices.

*   **Separation of Concerns**: The core logic is divided into two distinct agents:
    *   `PlanAgent`: Responsible only for high-level planning.
    *   `ImplAgent`: Responsible only for tactical execution of a plan.
*   **Generic Agent Trait**: A common `Agent` trait (`src/agents/mod.rs`) provides a consistent interface for all agents, making the system extensible.
*   **Decoupled Modules**: Functionality is organized into single-responsibility modules:
    *   `cli.rs`: CLI definition.
    *   `config.rs`: Configuration loading.
    *   `error.rs`: Centralized error handling.
    *   `files.rs`: File system operations.
    *   `vcs.rs`: Version control (Git) operations.
    *   `gemini.rs`: A wrapper for all Gemini API interactions.
    *   `agents/state.rs`: All serializable state structs.
*   **Structured State**: The application is stateless and relies on the filesystem (e.g., `.ai/plan_my_feature_1678886400.toml`) to store its state, allowing for resumable and auditable workflows.

## Testing Strategy

The project employs a multi-layered testing strategy to ensure correctness and reliability.

*   **Unit Tests**: Individual functions and components are tested in isolation. For example, `files.rs` is tested against a temporary directory structure to verify file filtering logic.
*   **Integration Tests**: The interaction between components is tested, mocking external services. The primary tool for this is `wiremock`, which simulates the Gemini API. This allows for deterministic testing of agent logic without making real API calls, verifying that agents send correctly formatted requests and correctly process responses.
*   **End-to-End (E2E) Tests**: The compiled binary is tested from a user's perspective using the `assert_cmd` crate. These tests cover the full `plan` and `impl` workflows, running against temporary project directories and a mocked API server. This ensures all components are correctly integrated and the CLI behaves as expected.

All tests are designed to run sequentially (`--test-threads=1`) to avoid race conditions when manipulating environment variables for the mock server URL.

## Robustness and Error Handling

Several mechanisms are in place to ensure the agent operates reliably and fails gracefully.

*   **Circuit Breakers for Loops**: The `ImplAgent` operates on a retry loop for each task. The `max_retries` parameter (configurable via the CLI) acts as a crucial circuit breaker, preventing infinite loops if a task consistently fails validation. If a task fails all retries, the agent halts and reports the failure.
*   **Structured API Interaction**: The system heavily relies on Gemini's **Function Calling** feature instead of parsing free-form text. This ensures that data from the LLM is structured and can be reliably deserialized into Rust structs.
*   **Handling Malformed Responses**: If the API returns a response that is not a valid function call or if the arguments do not match the expected JSON schema, `serde_json` will return a deserialization error. This error is caught and propagated through the application's central `Error` enum, causing the current operation to fail cleanly rather than panic.
*   **Network and API Errors**: The `gemini.rs` wrapper handles network issues and API errors.
    *   Connection errors from `reqwest` are caught and wrapped in `Error::Reqwest`.
    *   Non-successful HTTP status codes from the Gemini API are detected, and the error response is wrapped in `Error::Gemini`.
*   **Error Summarization**: For long validation error messages (e.g., verbose compiler output), the `ImplAgent` uses a smaller, faster LLM to summarize the error before feeding it back into the prompt for the next attempt. This prevents hitting the context window limit of the primary model and focuses the agent on the most relevant part of the error. This feature only activates for errors exceeding a certain length, avoiding unnecessary API calls.
*   **Error Enrichment with Google Search**: When the `--enrich-errors` flag is used, the `ImplAgent` can augment its error context with external information. If a task fails, the agent uses Google Search (prioritizing `docs.rs` and `crates.io`) to find documentation or examples related to the error. This research is then added to the prompt for the next retry, giving the agent more context to solve complex problems like dependency issues or obscure compiler errors.
