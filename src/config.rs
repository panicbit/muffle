use std::fs;
use std::path::Path;

use color_eyre::eyre::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub rules: Vec<Rule>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let config = fs::read_to_string(path)
            .with_context(|| format!("failed to open config `{path:?}`"))?;
        let config = toml::from_str::<Self>(&config).context("failed to parse config")?;

        Ok(config)
    }
}

#[derive(Deserialize)]
pub struct Rule {
    pub input_pattern: String,
    pub output_allow_pattern: String,
}
