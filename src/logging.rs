use color_eyre::eyre::{Context, Result};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

pub fn init() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .without_time()
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("failed to set default tracing subscriber")?;

    Ok(())
}
