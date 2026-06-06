use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, thread};

use color_eyre::eyre::{Context, Result};
use inotify::{Inotify, WatchMask};
use parking_lot::RwLock;
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::filter;

#[derive(Deserialize)]
pub struct Config {
    pub log_only: bool,
    pub unlink: Vec<filter::Expr>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let config = fs::read_to_string(path)
            .with_context(|| format!("failed to open config `{path:?}`"))?;
        let config = toml::from_str::<Self>(&config).context("failed to parse config")?;

        Ok(config)
    }

    pub fn watch(path: impl Into<PathBuf>) -> Result<Arc<RwLock<Self>>> {
        let path = path.into();
        let config = Self::load(&path).context("failed to load initial config")?;
        let config = Arc::new(RwLock::new(config));
        let mut inotify = Inotify::init().context("failed to initialize inotify")?;

        inotify
            .watches()
            .add(&path, WatchMask::MODIFY)
            .context("Failed to watch config file")?;

        {
            let config = Arc::clone(&config);

            thread::spawn(move || {
                let mut buffer = [0; 4096];

                loop {
                    let events = match inotify.read_events_blocking(&mut buffer) {
                        Ok(events) => events,
                        Err(err) => {
                            error!("Error reading inotify events: {err}");
                            warn!("Stopping config update.");
                            return;
                        }
                    };

                    let need_reload = events
                        // .inspect(|event| debug!("inotify: {event:?}"))
                        .last()
                        .is_some();

                    if need_reload {
                        info!("🔄 Reloading config...");

                        let new_config = match Self::load(&path) {
                            Ok(new_config) => new_config,
                            Err(err) => {
                                error!("Failed to load config: {err}");
                                continue;
                            }
                        };

                        *config.write() = new_config;
                        info!("Config reload successful.");
                    }
                }
            });
        }

        Ok(config)
    }
}
