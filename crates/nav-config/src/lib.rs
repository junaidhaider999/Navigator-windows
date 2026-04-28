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
    /// Global activation chord (see `nav-input` / `04-build-order.md` E4).
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    #[serde(default)]
    pub log: LogConfig,
    /// Overlay rendering (hint pills, diagnostics).
    #[serde(default)]
    pub render: RenderConfig,
    #[serde(default)]
    pub fallback: FallbackConfig,
}

/// Overlay pill rendering options (`config.toml` `[render]`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RenderConfig {
    /// Draw pill→target connector lines. Off by default (noisy); enable for debugging layout.
    #[serde(default)]
    pub debug_connectors: bool,
    /// Red dot at the resolved invoke anchor (physical point mapped to overlay DIPs).
    #[serde(default)]
    pub debug_target_dot: bool,
    /// Green outline around element bounding rects (UIA bounds).
    #[serde(default)]
    pub debug_target_rect: bool,
    /// Numeric distance from pill center to invoke anchor (DIPs).
    #[serde(default)]
    pub debug_distance: bool,
}

fn default_alphabet() -> String {
    "sadfjklewcmpgh".to_string()
}

fn default_max_elements() -> usize {
    2048
}

fn default_enumeration_profile() -> String {
    "fast".to_string()
}

fn default_materialize_budget_ms() -> u64 {
    30
}

fn default_hint_cache_ttl_ms() -> u64 {
    1000
}

fn default_planner_label_cap() -> usize {
    60
}

fn default_pipeline_soft_budget_ms() -> u64 {
    25
}

fn default_pipeline_hard_budget_ms() -> u64 {
    30
}

fn default_enumeration_ladder() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HintsConfig {
    /// Characters used for vimium-style labels, in priority order (whitespace ignored when ingested by the app).
    #[serde(default = "default_alphabet")]
    pub alphabet: String,
    /// Hard cap on enumerated / labeled elements per session.
    #[serde(default = "default_max_elements")]
    pub max_elements: usize,
    /// `fast` = patterns-only UIA match (low latency). `full` = broader keyboard-focusable matching.
    #[serde(default = "default_enumeration_profile")]
    pub enumeration_profile: String,
    /// Stop Rust-side materialization after this many milliseconds (partial hint list).
    #[serde(default = "default_materialize_budget_ms")]
    pub materialize_budget_ms: u64,
    /// Backend ordering: `auto` (class/exe probe), `uia_first`, `win32_first`, `chromium_fast`.
    #[serde(default = "default_enumeration_ladder")]
    pub enumeration_ladder: String,
    /// Re-use last enumeration for the same HWND/PID within this window (instant repeat activation).
    #[serde(default = "default_hint_cache_ttl_ms")]
    pub hint_cache_ttl_ms: u64,
    /// Log `[pipeline]` warning when hotkey→plan exceeds this many milliseconds.
    #[serde(default = "default_pipeline_soft_budget_ms")]
    pub pipeline_soft_budget_ms: u64,
    /// Hard ceiling for `[pipeline]` diagnostics (enumeration may still return partial results earlier).
    #[serde(default = "default_pipeline_hard_budget_ms")]
    pub pipeline_hard_budget_ms: u64,
    /// Max hints passed to the planner after priority ranking (`0` = no extra cap beyond enumeration).
    #[serde(default = "default_planner_label_cap")]
    pub planner_label_cap: usize,
}

impl Default for HintsConfig {
    fn default() -> Self {
        Self {
            alphabet: default_alphabet(),
            max_elements: default_max_elements(),
            enumeration_profile: default_enumeration_profile(),
            materialize_budget_ms: default_materialize_budget_ms(),
            enumeration_ladder: default_enumeration_ladder(),
            hint_cache_ttl_ms: default_hint_cache_ttl_ms(),
            pipeline_soft_budget_ms: default_pipeline_soft_budget_ms(),
            pipeline_hard_budget_ms: default_pipeline_hard_budget_ms(),
            planner_label_cap: default_planner_label_cap(),
        }
    }
}

fn default_hotkey_chord() -> String {
    "alt+/".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HotkeyConfig {
    /// e.g. `alt+/`, `ctrl+shift+a` (parsed by `nav-input`).
    #[serde(default = "default_hotkey_chord")]
    pub chord: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            chord: default_hotkey_chord(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LogConfig {
    #[serde(default)]
    pub level: Option<String>,
}

// M9 default stage budgets (ms) — must match `nav_uia::M9_DEFAULT_BUDGET_*_MS` and `m9-acceptance.md`.
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
        assert_eq!(
            parsed.hints.enumeration_profile,
            def.hints.enumeration_profile
        );
        assert_eq!(
            parsed.hints.materialize_budget_ms,
            def.hints.materialize_budget_ms
        );
        assert_eq!(
            parsed.hints.enumeration_ladder,
            def.hints.enumeration_ladder
        );
        assert_eq!(parsed.hints.hint_cache_ttl_ms, def.hints.hint_cache_ttl_ms);
        assert_eq!(
            parsed.hints.pipeline_soft_budget_ms,
            def.hints.pipeline_soft_budget_ms
        );
        assert_eq!(
            parsed.hints.pipeline_hard_budget_ms,
            def.hints.pipeline_hard_budget_ms
        );
        assert_eq!(parsed.hints.planner_label_cap, def.hints.planner_label_cap);
        assert_eq!(parsed.hotkey.chord, def.hotkey.chord);
        assert_eq!(parsed.fallback.budget_ms.uia, def.fallback.budget_ms.uia);
        assert_eq!(parsed.render.debug_connectors, def.render.debug_connectors);
        assert_eq!(parsed.render.debug_target_dot, def.render.debug_target_dot);
        assert_eq!(
            parsed.render.debug_target_rect,
            def.render.debug_target_rect
        );
        assert_eq!(parsed.render.debug_distance, def.render.debug_distance);
    }
}
