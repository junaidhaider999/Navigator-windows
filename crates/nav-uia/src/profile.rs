//! Hardcoded per-exe tuning (enumeration ladder supplements [`crate::strategy`] defaults).

use crate::options::{EnumOptions, EnumerationProfile};

/// Merge known-app defaults into options from config (profile does not replace user strategy_mode).
pub fn apply_exe_profile(exe_basename: &str, opts: &mut EnumOptions) {
    let e = exe_basename.to_ascii_lowercase();
    match e.as_str() {
        // Chromium — coverage-first: Full find condition + shallow Children pass must align with invoke.
        "chrome.exe" | "msedge.exe" | "brave.exe" | "vivaldi.exe" | "opera.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 6;
            opts.uia_shallow_materialize_budget_ms = 16;
        }
        "discord.exe" | "slack.exe" | "teams.exe" | "electron.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 6;
            opts.uia_shallow_materialize_budget_ms = 14;
        }
        "cursor.exe" | "code.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 8;
            opts.uia_shallow_materialize_budget_ms = 16;
        }
        "wispr flow.exe" | "wisprflow.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 6;
            opts.uia_shallow_materialize_budget_ms = 12;
        }
        "windowsterminal.exe" | "wt.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 6;
            opts.uia_shallow_materialize_budget_ms = 12;
        }
        "explorer.exe" => {
            opts.profile = EnumerationProfile::Full;
            opts.explorer_enrich_if_below = 45;
            opts.explorer_enrich_materialize_budget_ms = 48;
            opts.clip_uia_to_client_rect = true;
        }
        _ => {}
    }
}
