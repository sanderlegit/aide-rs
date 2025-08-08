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
    },
    /// Lists all available flows.
    List,
}
