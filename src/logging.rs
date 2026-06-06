use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use color_eyre::eyre::{Context, ContextCompat, Result};
use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{FmtSubscriber, fmt};

pub fn init() -> Result<()> {
    let terminal_subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .without_time()
        .with_target(false)
        .finish();
    let file_subscriber = fmt::layer()
        .with_target(false)
        .with_writer(Mutex::new(log_rotate_writer()?));

    tracing::subscriber::set_global_default(terminal_subscriber.with(file_subscriber))
        .context("failed to set default tracing subscriber")?;

    Ok(())
}

fn log_dir() -> Result<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com.github", "panicbit", "muffle")
        .context("failed to get project dirs")?;
    let state_dir = project_dirs
        .state_dir()
        .context("failed to get state dir")?;

    Ok(state_dir.to_path_buf())
}

fn log_rotate_writer() -> Result<impl Write> {
    let log_dir = log_dir().context("failed to get log dir")?;

    fs::create_dir_all(&log_dir).context("failed to create log dir")?;

    let log_path = log_dir.join("log.txt");

    Ok(FileRotate::new(
        log_path,
        AppendCount::new(3),
        ContentLimit::Lines(1_000),
        Compression::None,
        None,
    ))
}
