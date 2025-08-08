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
        #[arg(long, value_name = "PATH")]
        output_plan: Option<PathBuf>,
    },
    /// Executes an implementation plan.
    Impl {
        #[arg(short, long, value_name = "PATH")]
        plan: PathBuf,
        /// The maximum number of attempts per task. Overridden by `max_task_retries` in the plan file.
        #[arg(long, default_value_t = 5)]
        max_retries: u32,
        #[arg(long)]
        auto_commit: bool,
        /// Use the local `doc-retriever` tool to enrich error messages with rustdoc.
        #[arg(long)]
        enrich_errors: bool,
    },
}
