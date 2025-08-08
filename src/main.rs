use aide_rs::{
    cli::{Cli, Commands},
    error::Result,
    logging::RunLogger,
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
    let _logger = RunLogger::new()?;
    let cli = Cli::parse();

    info!(command = ?cli.command, "Executing command");

    match cli.command {
        Commands::Run { flow_name, prompt } => {
            info!(%flow_name, ?prompt, "Running flow");
            // TODO: Implement the FlowRunner logic
            // 1. Find and parse `flows/{flow_name}.yml`
            // 2. Parse the `--prompt` file
            // 3. Create a FlowRunner instance
            // 4. Execute the flow
            println!(
                "Executing flow '{}' with prompt '{}'",
                flow_name,
                prompt.display()
            );
            println!("(Flow runner not yet implemented)");
        }
        Commands::List => {
            info!("Listing available flows");
            // TODO: Implement logic to find and list all `*.yml` files in `flows/`
            println!("Available flows:");
            println!("(Flow listing not yet implemented)");
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
