use aide_rs::{
    agents::{impl_agent::ImplAgent, plan_agent::PlanAgent, state::PlanPrompt, Agent},
    cli::{Cli, Commands},
    error::Result,
};
use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter, FmtSubscriber};

fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
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
        Commands::Plan {
            prompt,
            output_plan,
        } => {
            info!(?prompt, ?output_plan, "Running Plan agent");

            // 1. Load prompt
            let prompt_content = std::fs::read_to_string(&prompt)?;
            let plan_prompt: PlanPrompt = toml::from_str(&prompt_content)?;

            // 2. Create and run agent
            let plan_agent = PlanAgent::new()?;
            let implementation_plan = plan_agent.run(plan_prompt).await?;

            // 3. Save plan
            let plan_json = serde_json::to_string_pretty(&implementation_plan)?;
            if let Some(parent) = output_plan.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output_plan, plan_json)?;

            info!("Implementation plan saved to {:?}", output_plan);
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
            let impl_agent = ImplAgent::new(max_retries, auto_commit)?;
            impl_agent.run(plan).await?;

            info!("Implementation agent finished successfully.");
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
