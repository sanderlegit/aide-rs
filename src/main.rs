use aide_rs::{
    cli::{Cli, Commands},
    error::{Error, Result},
    flows::types::Flow,
    logging::RunLogger,
    runner::FlowRunner,
};
use clap::Parser;
use std::{fs, path::PathBuf};
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

    info!(command = ?cli.command, "Executing command");

    match cli.command {
        Commands::Run { flow_name, prompt } => {
            let logger = RunLogger::new()?;
            info!(%flow_name, ?prompt, "Running flow");

            let flow_path = PathBuf::from(format!("flows/{}.yml", flow_name));
            if !flow_path.exists() {
                return Err(Error::Config(format!(
                    "Flow file not found: {}",
                    flow_path.display()
                )));
            }
            let flow_content = fs::read_to_string(&flow_path)?;
            let flow: Flow = serde_yaml::from_str(&flow_content)?;

            let mut runner = FlowRunner::new(logger)?;
            runner.run(&flow, &prompt).await?;
        }
        Commands::List => {
            info!("Listing available flows");
            println!("Available flows:");
            let flow_dir = PathBuf::from("flows");
            if flow_dir.is_dir() {
                for entry in fs::read_dir(flow_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext == "yml" || ext == "yaml" {
                                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                    let content = fs::read_to_string(&path).unwrap_or_default();
                                    let flow: std::result::Result<Flow, _> =
                                        serde_yaml::from_str(&content);
                                    match flow {
                                        Ok(f) => println!("- {}: {}", stem, f.description),
                                        Err(_) => {
                                            println!("- {} (could not parse description)", stem)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                println!("'flows' directory not found.");
            }
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
