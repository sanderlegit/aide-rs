use aide_rs::doc_retriever::{get_crate_docs, get_module_docs, get_type_docs};
use aide_rs::error::Result;
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser, Debug)]
#[command(
    name = "doc-retriever",
    about = "A tool to retrieve Rust documentation as structured JSON."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Get crate-level documentation.
    Crate {
        #[arg(long)]
        name: String,
    },
    /// Get module-level documentation.
    Module {
        #[arg(long = "crate")]
        crate_name: String,
        #[arg(long)]
        path: String,
    },
    /// Get type-level documentation (struct or enum).
    Type {
        #[arg(long = "crate")]
        crate_name: String,
        #[arg(long)]
        path: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Crate { name } => get_crate_docs(&name, None),
        Commands::Module { crate_name, path } => get_module_docs(&crate_name, &path, None),
        Commands::Type { crate_name, path } => get_type_docs(&crate_name, &path, None),
    };

    match result {
        Ok(json) => {
            println!("{}", serde_json::to_string_pretty(&json)?);
            Ok(())
        }
        Err(e) => {
            let json_err = json!({
                "success": false,
                "error": e.to_string(),
            });
            eprintln!("{}", serde_json::to_string_pretty(&json_err)?);
            std::process::exit(1);
        }
    }
}
