use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, thread};

use color_eyre::eyre::{Context, Result};
use debounce::EventDebouncer;
use inotify::{EventMask, Inotify, WatchMask};
use pipewire::channel::Receiver;
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

    pub fn watch(path: impl Into<PathBuf>) -> Result<(Config, Receiver<Config>)> {
        let path = path.into();
        let initial_config = Self::load(&path).context("failed to load initial config")?;
        let mut inotify = Inotify::init().context("failed to initialize inotify")?;
        let (tx, rx) = pipewire::channel::channel::<Config>();

        inotify
            .watches()
            .add(&path, WatchMask::MODIFY)
            .context("Failed to watch config file")?;

        let delay = Duration::from_millis(100);
        let debouncer = EventDebouncer::new(delay, {
            move |_event_mask: EventMask| {
                info!("🔄 Reloading config...");

                let new_config = match Self::load(&path) {
                    Ok(new_config) => new_config,
                    Err(err) => {
                        error!("Failed to load config: {err}");
                        return;
                    }
                };

                tx.send(new_config).ok();
            }
        });

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

                for event in events {
                    debouncer.put(event.mask);
                }
            }
        });

        Ok((initial_config, rx))
    }
}
