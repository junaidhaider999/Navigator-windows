//! Load merged configuration from TOML (`Agent/workflow/13-configuration.md`).
//!
//! Supports `--config <path>`; discovery of `%APPDATA%` / exe-dir is deferred to M10 shell work.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NavConfigError {
    #[error("failed to read config file {0}: {1}")]
    Io(String, std::io::Error),
    #[error("failed to parse config TOML: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub hints: HintsConfig,
}

fn default_alphabet() -> String {
    "sadfjklewcmpgh".to_string()
}

fn default_max_elements() -> usize {
    2048
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HintsConfig {
    /// Characters used for vimium-style labels, in priority order (whitespace ignored when ingested by the app).
    #[serde(default = "default_alphabet")]
    pub alphabet: String,
    /// Hard cap on enumerated / labeled elements per session.
    #[serde(default = "default_max_elements")]
    pub max_elements: usize,
}

impl Default for HintsConfig {
    fn default() -> Self {
        Self {
            alphabet: default_alphabet(),
            max_elements: default_max_elements(),
        }
    }
}

/// Load from an optional TOML file; if `path` is `None`, returns built-in defaults.
pub fn load(path: Option<&Path>) -> Result<Config, NavConfigError> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| NavConfigError::Io(path.display().to_string(), e))?;
    toml::from_str(&text).map_err(|e| NavConfigError::Parse(e.to_string()))
}

/// Alphabet as non-whitespace chars (same trimming the app has always applied in spirit).
#[must_use]
pub fn alphabet_chars(cfg: &Config) -> Vec<char> {
    cfg.hints
        .alphabet
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
