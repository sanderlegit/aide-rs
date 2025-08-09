# Architecture Refactor: From Agents to Declarative Flows

This document outlines a comprehensive architectural refactoring for `aide-rs`. The goal is to move from a rigid, two-agent system (`plan` and `impl`) to a flexible, declarative, and modular workflow engine driven by "Flows". This change will dramatically increase the tool's power, adaptability, and ease of extension.

## 1. Core Philosophy: Declarative Workflows

The new architecture is built on a single, powerful idea: **any automated development task can be described as a sequence of prompt-driven blocks.**

Instead of hardcoding agent logic in Rust, we will define workflows in simple YAML files. This makes the agent's "brain" transparent, easy to modify, and version-controllable alongside the code it operates on.

The core pillars of this new philosophy are:
1.  **Declarative over Imperative**: Define *what* the agent should do in YAML, not *how* it should do it in Rust.
2.  **Modularity and Composability**: Workflows ("Flows") are composed of smaller, reusable "Blocks".
3.  **Transparency and Control**: The entire logic of a workflow is visible in a single file, allowing for easy inspection, modification, and creation of new workflows without recompiling the application.

## 2. The Flow Execution Model

The new system revolves around three key concepts:

-   **Flow**: A complete, end-to-end workflow defined in a YAML file (e.g., `flows/plan.yml`). It has a name, a description, and consists of one or more Blocks.
-   **Block**: A single, atomic step within a Flow. A Block defines how to construct a prompt, how to execute it (e.g., with history, with tools), and how to validate its output.
-   **Flow Runner**: The Rust component (`src/runner.rs`) responsible for parsing a Flow YAML file and executing its Blocks sequentially, managing state, history, and tool calls.

A typical end-to-end `code` flow execution process is as follows:
1.  User invokes `aide-rs run code --prompt <initial_context.yml>`.
2.  The `Flow Runner` loads `flows/code.yml`.
3.  **Block 1: High-Level Planning.** The first block prompts the LLM to generate a detailed, human-readable implementation plan in Markdown format. This is a creative step to outline the strategy, including new files, tests, and logic changes.
4.  **Block 2: Structured Task Generation.** The Markdown plan from the previous block is used as input. This block's prompt asks the LLM to convert the plan into a structured list of high-level task descriptions (e.g., a `Vec<TaskDescription>`). This is achieved by calling a tool like `create_task_list`.
5.  **Block 3: Task Implementation Loop.** This block is managed by special logic within the `FlowRunner`. It iterates through the list of tasks generated in the previous step. The agent may decide to batch several related tasks together. For each task (or batch):
    a. **Just-in-Time (JIT) Planning:** A sub-prompt is run to create a `DetailedTaskPlan` for the current task batch. This plan is a structured object defining the specific file scopes, validation commands, and success criteria for *only this batch*.
    b. **Implementation:** Another sub-prompt is run to execute the `DetailedTaskPlan`. The LLM uses tools like `edit_file` to perform the work.
    c. **Verification:** The `verification` strategy for the block (e.g., `cargo check`) is executed. If it fails, the implementation step is retried with the error context, up to `max_retries`.
6.  The `FlowRunner` continues this loop until all tasks are successfully completed.

## 3. Flow Definition: The YAML Schema

Flows are defined in YAML files within the `flows/` directory.

**Example: `flows/plan.yml`**
```yaml
id: "plan"
description: "Analyzes an objective and creates a detailed, step-by-step implementation plan as a list of tasks."

blocks:
  - id: "generate_tasks"
    description: "Generate a high-level list of task descriptions."
    prompt:
      composition:
        - type: static_text
          content: "You are an expert software architect..." # System prompt
        - type: file_contents
          scopes: [ "base", "prompt" ] # Use the base scope and the one from the user prompt
          prefix: "**Project File Context:**\n"
        - type: prompt_file_field
          field: "objective"
          prefix: "**Objective:**\n"
        - type: prompt_file_field
          field: "coding_conventions"
          prefix: "**Coding Conventions:**\n"
    annotations:
      history: none # This is the first turn, no history needed
      tools: [ "task_creator" ]
      structured_output_schema: "TaskList" # Expects the LLM to call a tool that produces a TaskList
```

### 3.1. Block Definition

Each item in the `blocks` array is a Block object with the following structure:

-   `id` (string, required): A unique name for the block (e.g., `generate_tasks`, `implement_task_loop`).
-   `description` (string, optional): A human-readable explanation of the block's purpose.
-   `prompt` (object, required): Defines how to construct the prompt for the LLM.
-   `annotations` (object, optional): Modifies the execution behavior of the block.
-   `verification` (object, optional): Defines how to validate the block's output and whether to loop on failure.

### 3.2. Prompt Construction System (`prompt` object)

The `prompt` object defines the components that will be assembled into the final prompt sent to the LLM.

-   `composition` (array): A list of parts to concatenate. Each part is an object with a `type`:
    -   `type: static_text`: Simply includes a hardcoded string.
        -   `content`: The string to include.
    -   `type: file_contents`: Includes the content of files by merging a list of named scopes.
        -   `scopes`: A list of scope names. For each name, the runner loads `ctx/<name>.yaml`. The special name `"prompt"` refers to the `file_scoping` key from the user's initial `.yml` prompt file. Scopes are merged in order, with later scopes overriding earlier ones.
        -   `prefix`: A string to prepend to the file context.
    -   `type: prompt_file_field`: Includes a specific field from the user's initial prompt file.
        -   `field`: The name of the field (e.g., `objective`).
        -   `prefix`: A string to prepend to the field's content.
    -   `type: previous_output`: Includes the output from a previous block.
        -   `block_id`: The `id` of the block whose output to include.
        -   `prefix`: A string to prepend to the output.

### 3.3. Block Annotations (`annotations` object)

Annotations control the "runtime" behavior of a block.

-   `history` (string or object): How much of the conversation history to include.
    -   `full` (default): Include the entire history from the start of the Flow.
    -   `none`: Do not include any prior history.
    -   `last_n: <number>`: Include only the last N prompt/response pairs.
-   `tools` (array of strings): A list of toolsets available to the LLM for this block.
    -   `file_system`: `edit_file`, `create_file`.
    -   `doc_retriever`: The `doc_retriever` tool.
    -   `google_search`: Enables grounding with Google Search.
    -   `task_creator`: A tool to define a structured list of tasks.
-   `structured_output_schema` (string, optional): The name of a Rust struct that the LLM's output (likely from a tool call) should be deserialized into. This enables strongly-typed outputs.

#### 3.3.1. Tool Constraints

To ensure predictable behavior, the `FlowRunner` enforces constraints on which tools can be used together in a single block. These are mutually exclusive and will cause a validation error if combined:

-   **Research vs. Modification**: A block cannot have both `google_search` and `file_system` tools enabled. Research is for information gathering, while file system operations are for implementation. This separation prevents the agent from making file changes based on potentially unverified web content in the same step. If file changes are needed after research, they should be done in a subsequent block.

### 3.4. Verification (`verification` object)

The verification step runs after a block completes, enabling loops and self-correction.

-   `max_retries` (integer, default: 5): How many times to loop on verification failure.
-   `strategy`: The verification method.
    -   `type: command`: Run a shell command.
        -   `command`: The command to run.
        -   `expected_exit_code`: The code for success.
        -   `on_failure_prompt`: A `prompt` object (see above) to construct the prompt for the next attempt, which will automatically include the command's output.
    -   `type: prompt`: Use the LLM to verify its own output.
        -   `prompt`: A `prompt` object to construct the verification prompt.
        -   `success_condition`: A condition for breaking the loop, e.g., a specific function call like `verification_passed()`.

### 3.5. Looping Strategy (`looping` object)

For blocks that need to iterate over a list of items (e.g., implementing a list of tasks), you can define a `looping` strategy. This is handled by special logic in the `FlowRunner`.

-   `over` (string, required): The `id` of a previous block whose output is a list (e.g., a `TaskList`).
-   `as` (string, required): The key to use when making the current item from the list available to the prompt composition system. For example, if `as: "current_task"`, you could use a `previous_output` part with `block_id: "current_task"`.
-   `clear_history_on_iteration` (boolean, default: false): If `true`, the conversation history is cleared at the beginning of each loop iteration. This is useful for long-running tasks to prevent the context from growing too large and to keep the agent focused on the current item.

## 4. Core Component Refactoring

This new architecture requires significant changes to the codebase.

-   **CLI (`src/cli.rs`, `src/main.rs`)**: The CLI will be simplified to `aide-rs run <flow>` and `aide-rs list`. The `main` function will dispatch to the `FlowRunner`.
-   **Flows (`src/flows/`)**: A new directory and module.
    -   `types.rs`: Contains the Rust structs that model the YAML schema (`Flow`, `Block`, `PromptComposition`, etc.).
    -   `mod.rs`: Publicly exports the types.
-   **Runner (`src/runner.rs`)**: A new module containing the `FlowRunner` struct. This will be the heart of the application, containing the logic to parse and execute flows.
-   **Tools (`src/tools.rs`)**: A new module to define the available tools (`edit_file`, `create_file`, `doc_retriever`, etc.) and their `FunctionDeclaration` schemas. The `FlowRunner` will use this module to provide the correct tools to the LLM based on a block's annotations.
-   **Deprecation**:
    -   `src/agents/plan_agent.rs` and `src/agents/impl_agent.rs` will be deleted.
    -   `src/agents/mod.rs` will be deleted.
    -   `src/agents/state.rs` will be replaced by `src/flows/types.rs`.

## 5. Migration Path: Replicating `plan` and `impl`

The previous `plan` and `impl` functionality can be replicated as two separate flows.

-   **`flows/plan.yml`**: A single-block flow that uses a prompt and the `task_creator` tool to generate a structured list of tasks, similar to the old `PlanAgent`.
-   **`flows/code.yml`**: A multi-stage flow that orchestrates the entire development process:
    1.  **High-Level Plan Block**: Takes the user's objective and generates a detailed implementation plan in Markdown.
    2.  **Structured Task Block**: Takes the Markdown plan and converts it into a structured list of task descriptions using a tool.
    3.  **Implementation Loop Block**: This block is designed to be looped over by the `FlowRunner`. For each task description from the previous block (or a batch of them), it performs a two-step process:
        -   **JIT Planning**: A prompt that takes the task description(s) and creates a detailed, structured `TaskPlan` (defining scopes, validation, etc.) for that specific work item.
        -   **Implementation & Verification**: A prompt that takes the `TaskPlan` and implements the code, followed by a `verification` step (e.g., `command: "cargo check"`) to ensure correctness. The `FlowRunner` handles the retry loop on failure.

This demonstrates the power of the new system: core behaviors are no longer hardcoded but are flexible, composable workflows.
