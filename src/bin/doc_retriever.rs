use aide_rs::doc_retriever::{get_crate_docs, get_item_docs};
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
    /// Get documentation for a specific item (module, struct, enum).
    Item {
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
        Commands::Item { crate_name, path } => get_item_docs(&crate_name, &path, None),
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
