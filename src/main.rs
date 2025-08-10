use aide_rs::{
    cli::{Cli, Commands},
    error::Result,
    file_provider,
    orchestrator::Orchestrator,
};
use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter, FmtSubscriber};

fn setup_logging() {
    let is_test_mode = std::env::var("AIDE_RS_TEST_MODE").is_ok();
    let subscriber = FmtSubscriber::builder()
        // Disable ANSI colors when running tests, as they can interfere with test output parsing.
        .with_ansi(!(cfg!(test) || is_test_mode))
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_span_events(FmtSpan::CLOSE)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let orchestrator = Orchestrator::new()?;

    info!(command = ?cli.command, "Executing command");

    match cli.command {
        Commands::Research { objective, files } => {
            let filtered_files = file_provider::get_files(&files, None)?;
            orchestrator.research(objective, filtered_files).await?;
        }
        Commands::Plan { objective, files } => {
            let filtered_files = file_provider::get_files(&files, None)?;
            let _ = orchestrator.plan(objective, filtered_files, true).await?;
        }
        Commands::Implement {
            objective,
            files,
            validate_cmd,
            auto,
        } => {
            let filtered_files = file_provider::get_files(&files, None)?;
            orchestrator
                .implement(objective, filtered_files, validate_cmd, auto)
                .await?;
        }
        Commands::Run { prompt_file } => {
            orchestrator.run(prompt_file).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    setup_logging();
    if let Err(e) = run().await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
