use aide_rs::{
    cli::{Cli, Commands},
    error::{Error, Result},
    files,
    flows::types::{FileScope, Flow},
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
        Commands::Run {
            flow_name,
            prompt,
            input_file,
            input_id,
        } => {
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

            if let (Some(file_path), Some(id)) = (input_file, input_id) {
                let content = fs::read_to_string(file_path)?;
                let json_value: serde_json::Value = serde_json::from_str(&content)?;
                runner.load_input(&id, json_value);
            }

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
        Commands::Show { flow_name } => {
            let mut flow_path = PathBuf::from(format!("flows/{}.yml", flow_name));
            if !flow_path.exists() {
                flow_path = PathBuf::from(format!("flows/{}.yaml", flow_name));
            }

            if !flow_path.exists() {
                return Err(Error::Config(format!(
                    "Flow '{}' not found. Looked for .yml and .yaml extensions in 'flows/'.",
                    flow_name
                )));
            }

            let flow_content = fs::read_to_string(&flow_path)?;
            let flow: Flow = serde_yaml::from_str(&flow_content)?;

            println!("Flow: {}", flow.id);
            println!("Description: {}", flow.description);
            println!("\n--- Definition (from {}) ---", flow_path.display());
            println!("{}", flow_content);
        }
        Commands::ListFiles { scope_names } => {
            let mut final_scope = FileScope::default();
            let base_dir = PathBuf::from(".");

            for scope_name in &scope_names {
                let scope_path = PathBuf::from(format!("ctx/{}.yaml", scope_name));
                if !scope_path.exists() {
                    eprintln!(
                        "Warning: Scope file not found, skipping: {}",
                        scope_path.display()
                    );
                    continue;
                }
                let scope = FileScope::from_yaml_file(&scope_path)?;
                final_scope.merge(scope);
            }

            let files = files::get_filtered_files(&base_dir, &final_scope)?;
            if files.is_empty() {
                println!("No files match the scope(s): {}", scope_names.join(", "));
            } else {
                println!(
                    "Files included in scope(s) [{}]:",
                    scope_names.join(", ")
                );
                let canonical_base_dir = base_dir.canonicalize()?;
                for file in files {
                    if let Ok(relative_path) = file.strip_prefix(&canonical_base_dir) {
                        println!("- {}", relative_path.display());
                    } else {
                        println!("- {}", file.display());
                    }
                }
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
