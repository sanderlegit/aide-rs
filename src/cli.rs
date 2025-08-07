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
