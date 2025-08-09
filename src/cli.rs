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
    /// Executes a predefined workflow (a "flow").
    Run {
        /// The name of the flow to run (e.g., "plan", "code", "research").
        #[arg(value_name = "FLOW_NAME")]
        flow_name: String,

        /// Path to the initial prompt or context file for the flow.
        #[arg(short, long, value_name = "PATH")]
        prompt: PathBuf,

        /// Path to a JSON file to load as input for a block.
        #[arg(long, value_name = "PATH")]
        input_file: Option<PathBuf>,

        /// The block ID to associate with the input file's content.
        #[arg(long, value_name = "ID", requires = "input_file")]
        input_id: Option<String>,

        /// Overrides the model specified in the flow file.
        #[arg(long, value_name = "MODEL_NAME")]
        model: Option<String>,
    },
    /// Lists all available flows.
    List,
    /// Shows the definition of a specific flow.
    Show {
        /// The name of the flow to show.
        #[arg(value_name = "FLOW_NAME")]
        flow_name: String,
    },
    /// Lists all files included in one or more context scopes.
    ListFiles {
        /// The names of the context scopes to use (e.g., "base", "ai").
        #[arg(required = true)]
        scope_names: Vec<String>,
    },
}
