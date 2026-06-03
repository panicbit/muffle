use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, thread};

use ::regex::Regex;
use color_eyre::eyre::{Context, Result};
use inotify::{Inotify, WatchMask};
use parking_lot::RwLock;
use serde::Deserialize;

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
                            eprintln!("Error reading inotify events: {err}");
                            eprintln!("Stopping config update.");
                            return;
                        }
                    };

                    let need_reload = events
                        // .inspect(|event| eprintln!("inotify: {event:?}"))
                        .last()
                        .is_some();

                    if need_reload {
                        eprintln!("🔄 Reloading config...");

                        let new_config = match Self::load(&path) {
                            Ok(new_config) => new_config,
                            Err(err) => {
                                eprintln!("Failed to load config: {err}");
                                continue;
                            }
                        };

                        *config.write() = new_config;
                        eprintln!("Config reload successful.");
                    }
                }
            });
        }

        Ok(config)
    }
}

#[derive(Deserialize)]
pub struct Rule {
    #[serde(with = "regex")]
    pub input_pattern: Regex,
    #[serde(with = "regex")]
    pub output_allow_pattern: Regex,
}

mod regex {
    use regex::Regex;
    use serde::{Deserialize, Deserializer, de};

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Regex, D::Error> {
        let pattern = String::deserialize(de)?;
        let regex = Regex::new(&pattern).map_err(de::Error::custom)?;

        Ok(regex)
    }
}
