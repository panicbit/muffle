use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use color_eyre::eyre::{Context, ContextCompat, Result};
use file_rotate::compression::Compression;
use file_rotate::suffix::AppendCount;
use file_rotate::{ContentLimit, FileRotate};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, fmt};

pub fn init() -> Result<()> {
    let terminal = fmt::layer()
        .without_time()
        .with_target(false)
        .with_ansi_sanitization(false);
    let file = fmt::layer()
        .with_target(false)
        .with_writer(Mutex::new(log_rotate_writer()?))
        .with_ansi_sanitization(false);
    let env = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let subscriber = tracing_subscriber::registry()
        .with(terminal)
        .with(file)
        .with(env);

    tracing::subscriber::set_global_default(subscriber)
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
