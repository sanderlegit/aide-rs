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
        Commands::Research {
            objective,
            files,
            context,
        } => {
            let files_to_provide = if let Some(context_name) = &context {
                file_provider::get_files(&[".".to_string()], Some(context_name), None)?
            } else if !files.is_empty() {
                file_provider::get_files(&files, None, None)?
            } else {
                return Err(aide_rs::error::Error::Config(
                    "You must provide either a list of files or a --context flag.".to_string(),
                ));
            };
            orchestrator.research(objective, files_to_provide).await?;
        }
        Commands::Plan {
            objective,
            files,
            context,
        } => {
            let files_to_provide = if let Some(context_name) = &context {
                file_provider::get_files(&[".".to_string()], Some(context_name), None)?
            } else if !files.is_empty() {
                file_provider::get_files(&files, None, None)?
            } else {
                return Err(aide_rs::error::Error::Config(
                    "You must provide either a list of files or a --context flag.".to_string(),
                ));
            };
            let _ = orchestrator
                .plan(objective, files_to_provide, true)
                .await?;
        }
        Commands::Implement {
            objective,
            files,
            validate_cmd,
            auto,
            context,
        } => {
            let files_to_provide = if let Some(context_name) = &context {
                file_provider::get_files(&[".".to_string()], Some(context_name), None)?
            } else if !files.is_empty() {
                file_provider::get_files(&files, None, None)?
            } else {
                return Err(aide_rs::error::Error::Config(
                    "You must provide either a list of files or a --context flag.".to_string(),
                ));
            };
            orchestrator
                .implement(objective, files_to_provide, validate_cmd, auto)
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
