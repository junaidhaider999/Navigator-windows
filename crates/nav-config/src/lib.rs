//! Load merged configuration from TOML (`Agent/workflow/13-configuration.md`).
//!
//! Discovery when no `--config`: `NAVIGATOR_CONFIG` → `%APPDATA%\\Navigator\\config.toml` →
//! `<exe-dir>\\config.toml` → built-in defaults.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NavConfigError {
    #[error("failed to read config file {0}: {1}")]
    IoRead(String, io::Error),
    #[error("failed to write config file {0}: {1}")]
    IoWrite(String, io::Error),
    #[error("failed to parse config TOML: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub hints: HintsConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub fallback: FallbackConfig,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LogConfig {
    #[serde(default)]
    pub level: Option<String>,
}

fn default_budget_uia() -> u64 {
    25
}
fn default_budget_msaa() -> u64 {
    8
}
fn default_budget_hwnd() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BudgetMs {
    #[serde(default = "default_budget_uia")]
    pub uia: u64,
    #[serde(default = "default_budget_msaa")]
    pub msaa: u64,
    #[serde(default = "default_budget_hwnd")]
    pub hwnd: u64,
}

impl Default for BudgetMs {
    fn default() -> Self {
        Self {
            uia: default_budget_uia(),
            msaa: default_budget_msaa(),
            hwnd: default_budget_hwnd(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct FallbackConfig {
    /// Per-stage soft time budgets in milliseconds (logged when exceeded in `nav-uia`).
    #[serde(default)]
    pub budget_ms: BudgetMs,
}

/// Load a single file (used when the path is known to exist).
pub fn load(path: Option<&Path>) -> Result<Config, NavConfigError> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| NavConfigError::IoRead(path.display().to_string(), e))?;
    toml::from_str(&text).map_err(|e| NavConfigError::Parse(e.to_string()))
}

/// Ordered discovery list (first existing file should win). `cli` path is listed first when set.
#[must_use]
pub fn discovery_candidates(cli: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = cli {
        out.push(p.to_path_buf());
    }
    if let Ok(s) = std::env::var("NAVIGATOR_CONFIG") {
        let p = PathBuf::from(s.trim());
        if !out.iter().any(|x| x == &p) {
            out.push(p);
        }
    }
    if let Some(p) = appdata_config_path() {
        if !out.iter().any(|x| x == &p) {
            out.push(p);
        }
    }
    if let Some(p) = exe_dir_config_path() {
        if !out.iter().any(|x| x == &p) {
            out.push(p);
        }
    }
    out
}

/// Load first existing path from [`discovery_candidates`], else defaults.
pub fn load_discovered(cli_override: Option<&Path>) -> Result<Config, NavConfigError> {
    for p in discovery_candidates(cli_override) {
        if p.exists() {
            return load(Some(p.as_path()));
        }
    }
    Ok(Config::default())
}

/// Like `load_discovered`, but if `cli` points to a path that does not exist, return an error
/// (explicit `--config` must exist).
pub fn load_for_startup(cli: Option<&Path>) -> Result<Config, NavConfigError> {
    if let Some(p) = cli {
        if !p.exists() {
            return Err(NavConfigError::IoRead(
                p.display().to_string(),
                io::Error::new(io::ErrorKind::NotFound, "config file not found"),
            ));
        }
        return load(Some(p));
    }
    load_discovered(None)
}

/// `%APPDATA%\\Navigator\\config.toml` (file may not exist yet).
#[must_use]
pub fn appdata_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("Navigator").join("config.toml"))
}

fn exe_dir_config_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("config.toml"))
}

/// Preferred path for [`write_default_config`] when no `--config` is passed.
#[must_use]
pub fn default_user_config_path() -> PathBuf {
    appdata_config_path().unwrap_or_else(|| PathBuf::from("config.toml"))
}

/// Serialize defaults for `--reset-config`.
pub fn default_config_toml() -> Result<String, NavConfigError> {
    let c = Config::default();
    toml::to_string_pretty(&c).map_err(|e| NavConfigError::Parse(e.to_string()))
}

pub fn write_default_config(path: &Path) -> Result<(), NavConfigError> {
    let text = default_config_toml()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| NavConfigError::IoWrite(parent.display().to_string(), e))?;
    }
    std::fs::write(path, text.as_bytes())
        .map_err(|e| NavConfigError::IoWrite(path.display().to_string(), e))
}

#[must_use]
pub fn alphabet_chars(cfg: &Config) -> Vec<char> {
    cfg.hints
        .alphabet
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_toml() {
        let s = default_config_toml().expect("default toml");
        let parsed: Config = toml::from_str(&s).expect("parse");
        let def = Config::default();
        assert_eq!(parsed.hints.alphabet, def.hints.alphabet);
        assert_eq!(parsed.hints.max_elements, def.hints.max_elements);
        assert_eq!(parsed.fallback.budget_ms.uia, def.fallback.budget_ms.uia);
    }
}
