use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "An AI-powered software development agent that orchestrates `aider`."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Launch a research session to investigate a topic and produce documentation.
    Research {
        /// The research topic (e.g., "best rust crates for audio processing").
        objective: String,
        /// Files to include in the context. Can be used if --context is not provided.
        files: Vec<String>,
        /// The context name for file filtering (e.g., 'all', 'backend'). Overrides `files`.
        #[arg(long)]
        context: Option<String>,
    },
    /// Launch a planning session to break an objective into a task list.
    Plan {
        /// The high-level goal to be planned.
        objective: String,
        /// Files to include in the context. Can be used if --context is not provided.
        files: Vec<String>,
        /// The context name for file filtering (e.g., 'all', 'backend'). Overrides `files`.
        #[arg(long)]
        context: Option<String>,
    },
    /// Launch an implementation session to work on code.
    Implement {
        /// The task to implement.
        objective: String,
        /// Files to include in the context. Can be used if --context is not provided.
        files: Vec<String>,
        /// The context name for file filtering (e.g., 'all', 'backend'). Overrides `files`.
        #[arg(long)]
        context: Option<String>,
        /// The command to run to validate changes.
        #[arg(long, default_value = "make test")]
        validate_cmd: String,
        /// Run in a fully automated loop, attempting to fix errors until validation passes.
        #[arg(long)]
        auto: bool,
        /// The maximum number of retries for the automated loop.
        #[arg(long, default_value = "5")]
        max_retries: u32,
    },
    /// Execute a non-interactive, multi-stage workflow from a config file.
    Run {
        /// A YAML file defining the objective and configuration for the run.
        prompt_file: String,
    },
}
