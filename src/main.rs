use aide_rs::error::Result;
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
    info!("Hello, world!");
    Ok(())
}
