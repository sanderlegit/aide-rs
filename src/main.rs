use aide_rs::{
    cli::{Cli, Commands},
    error::Result,
};
use clap::Parser;
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();
    let cli = Cli::parse();

    match cli.command {
        Commands::Plan {
            prompt,
            output_plan,
        } => {
            info!(?prompt, ?output_plan, "Running Plan agent");
            // Placeholder for PlanAgent logic
        }
        Commands::Impl {
            plan,
            max_retries,
            auto_commit,
        } => {
            info!(
                ?plan,
                ?max_retries,
                ?auto_commit,
                "Running Impl agent"
            );
            // Placeholder for ImplAgent logic
        }
    }

    Ok(())
}
