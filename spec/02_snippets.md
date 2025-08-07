### 2. `clap`: Command-Line Interface

`clap` parses command-line arguments into strongly-typed structs.

**Initialization (`cli.rs` and `main.rs`)**

Define your CLI structure using `derive` macros.

```rust
// In src/cli.rs
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "A Rust-based AI agent for automated software development.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generates a structured implementation plan.
    Plan {
        #[arg(short, long, value_name = "PATH")]
        prompt: PathBuf,
        #[arg(long, default_value = ".ai/implementation_plan.json")]
        output_plan: PathBuf,
    },
    /// Executes an implementation plan.
    Impl {
        #[arg(short, long, value_name = "PATH")]
        plan: PathBuf,
        #[arg(long, default_value_t = 5)]
        max_retries: u32,
        #[arg(long)]
        auto_commit: bool,
    },
}
```

**Usage (`main.rs`)**

Parse the arguments at the start of your application and use a `match` statement to dispatch to the correct agent logic.

```rust
// In src/main.rs
// use crate::cli::{Cli, Commands}; // Assuming these are in another file
// use clap::Parser;

// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     let cli = Cli::parse();
//
//     match cli.command {
//         Commands::Plan { prompt, output_plan } => {
//             println!("Running PlanAgent with prompt: {:?}", prompt);
//             // plan_agent::run(prompt, output_plan).await?;
//         }
//         Commands::Impl { plan, max_retries, auto_commit } => {
//             println!("Running ImplAgent with plan: {:?}", plan);
//             // impl_agent::run(plan, max_retries, auto_commit).await?;
//         }
//     }
//
//     Ok(())
// }
```

**Cleanup**

No cleanup is necessary. The `Cli` struct goes out of scope when it's no longer needed.

---

### 3. `serde` & `serde_json` / `toml`: State Management

`serde` is used to serialize your state structs into JSON for the implementation plan and deserialize them from TOML for the initial prompt.

**Initialization (`agents/state.rs`)**

Add `#[derive(Serialize, Deserialize)]` to your data model structs.

```rust
// In agents/state.rs
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PlanPrompt {
    pub objective: String,
    // ... other fields
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ImplementationPlan {
    pub tasks: Vec<Task>,
    // ... other fields
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    pub description: String,
    // ... other fields
}
```

**Usage (Loading a prompt and saving a plan)**

Use `fs::read_to_string` combined with `toml::from_str` or `serde_json::from_str` to load state, and `serde_json::to_string_pretty` to save it.

```rust
use std::fs;
use std::path::Path;
// use crate::agents::state::{PlanPrompt, ImplementationPlan}; // Your structs

// --- TOML Deserialization (for PlanPrompt) ---
fn load_prompt(path: &Path) -> Result<PlanPrompt, Box<dyn std::error::Error>> {
    let toml_content = fs::read_to_string(path)?;
    let prompt: PlanPrompt = toml::from_str(&toml_content)?; [18, 34]
    Ok(prompt)
}

// --- JSON Serialization (for ImplementationPlan) ---
fn save_plan(plan: &ImplementationPlan, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let json_content = serde_json::to_string_pretty(plan)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, json_content)?;
    Ok(())
}
```

**Cleanup**

No explicit cleanup is needed. File handles are closed automatically when they go out of scope.

---

### 4. `tracing` & `tracing-subscriber`: Logging

This pair provides structured, configurable logging.

**Initialization (`main.rs`)**

Set up a global subscriber once at the beginning of your application. The `EnvFilter` allows configuring log levels via the `RUST_LOG` environment variable.

```rust
// In src/main.rs
use tracing_subscriber::{EnvFilter, FmtSubscriber};

fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
        // Default to "info" level if RUST_LOG is not set.
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");
}

// Call this at the start of your main function.
// #[tokio::main]
// async fn main() -> Result<(), Box<dyn std::error::Error>> {
//     setup_logging();
//     // ... rest of the app
//     Ok(())
// }
```

**Usage (Throughout the application)**

Use the `info!`, `warn!`, `error!`, and `debug!` macros to log events.

```rust
use tracing::{info, warn, error, debug};

fn process_task(task_id: u32) {
    info!(task_id, "Starting to process task");

    if task_id == 0 {
        warn!(task_id, "This is a dummy task, might not do anything useful.");
    }
    
    debug!(task_id, "Task details: { some_detail: '...' }");

    if task_id > 100 {
      error!(task_id, "Invalid task ID encountered!");
      return;
    }

    info!(task_id, "Finished processing task");
}
```

**Cleanup**

No cleanup is required. The subscriber lives for the duration of the program.

---

### 5. `ignore`: File Filtering

The `ignore` crate efficiently finds files based on glob patterns while respecting `.gitignore`.

**Initialization and Usage (`files.rs`)**

Create a `WalkBuilder` and configure it with your include and exclude glob patterns from the `FileScope`.

```rust
// In src/files.rs
// use crate::agents::state::FileScope; // Your struct
// use crate::error::Result; // Your project's result type
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

fn get_filtered_files(base_dir: &Path, scope: &FileScope) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    let mut builder = WalkBuilder::new(base_dir);

    // Respect .gitignore but not other ignore files (.ignore, .rgignore)
    // and don't ignore hidden files by default.
    builder.hidden(false);

    let mut override_builder = ignore::overrides::OverrideBuilder::new(base_dir);
    for pattern in &scope.include {
        override_builder.add(pattern)?;
    }
    for pattern in &scope.exclude {
        let negated_pattern = format!("!{}", pattern);
        override_builder.add(&negated_pattern)?;
    }
    let overrides = override_builder.build()?;
    builder.overrides(overrides);

    for result in builder.build() {
        let entry = result?;
        if entry.file_type().map_or(false, |ft| ft.is_file()) {
            files.push(entry.into_path());
        }
    }
    files.sort(); // For deterministic order
    Ok(files)
}
```

**Cleanup**

The iterator consumes the `WalkBuilder`, and no manual cleanup is needed.

---

### 6. `git2`: Version Control System

The `git2` crate provides bindings for libgit2 to perform Git operations.

**Initialization**

Open the repository from the current working directory.

```rust
// In vcs.rs
use git2::{Repository, Signature, Oid, Commit, Tree};
use std::path::Path;

fn open_repository() -> Result<Repository, git2::Error> {
    Repository::open(".")
}
```

**Usage (Creating a commit)**

This is a multi-step process: find the current `HEAD`, create an index, add files to it, write the index as a tree, and finally, create the commit.

```rust
fn add_and_commit(repo: &Repository, paths: &[&Path], message: &str) -> Result<Oid, git2::Error> {
    let mut index = repo.index()?;

    // 1. Add specified files to the index
    for path in paths {
        index.add_path(path)?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // 2. Find the parent commit (HEAD)
    let head = repo.head()?.peel_to_commit()?;
    
    // 3. Create the commit
    let signature = Signature::now("AI Agent", "ai-agent@example.com")?;
    
    repo.commit(
        Some("HEAD"), // Update HEAD to point to the new commit
        &signature,   // Author
        &signature,   // Committer
        message,
        &tree,
        &[&head],     // Parents
    )
}
```

**Cleanup**

The `Repository` object and other Git-related structs will release their resources when they are dropped. No manual cleanup is needed.

---

### 7. `gemini_client_rs`: Gemini API Interaction

This is the core library for communicating with the LLM.

**Initialization (`gemini.rs`)**

Create a client. The API key is typically loaded from an environment variable.

```rust
// In gemini.rs
use gemini_client_rs::GeminiClient;
use std::env;

fn initialize_client() -> Result<GeminiClient, Box<dyn std::error::Error>> {
    // The client will automatically look for the GEMINI_API_KEY environment variable. [3]
    let client = GeminiClient::new()?;
    Ok(client)
}
```

**Usage (Making a function call request)**

Construct a request with a system prompt, user message, and tool definitions, then send it to the API.

```rust
use gemini_client_rs::types::{
    Content, Part, GenerateContentRequest,
    Tool, FunctionDeclaration, Schema, Type,
};
use serde_json::json;

async fn generate_plan(
    client: &GeminiClient,
    objective: String,
    file_list: String,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Define the function the LLM can call
    let create_plan_func = FunctionDeclaration {
        name: "create_implementation_plan".to_string(),
        description: "Creates a detailed, step-by-step implementation plan.".to_string(),
        parameters: Some(Schema {
            schema_type: Type::Object,
            properties: Some(
                [(
                    "tasks".to_string(),
                    Box::new(Schema {
                        schema_type: Type::Array,
                        items: Some(Box::new(Schema {
                            schema_type: Type::Object,
                            properties: Some(
                                [
                                    ("description".to_string(), Box::new(Schema { schema_type: Type::String, ..Default::default() })),
                                    // ... other Task fields
                                ].into(),
                            ),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }),
                )]
                .into(),
            ),
            ..Default::default()
        }),
    };

    let tools = vec![Tool {
        function_declarations: vec![create_plan_func],
    }];
    
    // 2. Construct the prompt
    let system_prompt = Part::text("You are an expert software architect...");
    let user_prompt = Part::text(format!(
        "Objective: {}\n\nFiles in scope:\n{}\n\nGenerate the plan.",
        objective, file_list
    ));

    let contents = vec![
        Content::new(vec![system_prompt]),
        Content::new(vec![user_prompt]),
    ];

    let request = GenerateContentRequest {
        contents,
        tools: Some(tools),
        ..Default::default()
    };
    
    // 3. Call the API and handle the response
    let response = client.generate_content(request).await?;
    
    if let Some(function_call) = response.candidates.get(0).and_then(|c| c.content.parts.get(0)?.as_function_call()) {
        println!("LLM wants to call function: {}", function_call.name);
        println!("With arguments:\n{}", function_call.args);
        
        // Here you would deserialize function_call.args into your ImplementationPlan struct
    } else {
        println!("Error: LLM did not return a function call.");
    }
    
    Ok(())
}
```

**Cleanup**

The `GeminiClient` handles its own HTTP connection pooling. There is no manual cleanup required.
