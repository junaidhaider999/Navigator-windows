//! Hardcoded per-exe tuning (enumeration ladder supplements [`crate::strategy`] defaults).

use crate::options::EnumOptions;

/// Merge known-app defaults into options from config (profile does not replace user strategy_mode).
pub fn apply_exe_profile(exe_basename: &str, opts: &mut EnumOptions) {
    let e = exe_basename.to_ascii_lowercase();
    match e.as_str() {
        // Chromium family — shallow Children FindAll first; deep scan only if thin.
        "chrome.exe" | "msedge.exe" | "brave.exe" | "vivaldi.exe" | "opera.exe" => {
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 10;
            opts.uia_shallow_materialize_budget_ms = 12;
        }
        "discord.exe" | "slack.exe" | "teams.exe" | "electron.exe" => {
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 8;
            opts.uia_shallow_materialize_budget_ms = 12;
        }
        "cursor.exe" | "code.exe" => {
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 12;
            opts.uia_shallow_materialize_budget_ms = 14;
        }
        "wispr flow.exe" | "wisprflow.exe" => {
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 8;
            opts.uia_shallow_materialize_budget_ms = 12;
        }
        "windowsterminal.exe" | "wt.exe" => {
            opts.uia_shallow_children_first = true;
            opts.uia_shallow_min_targets = 6;
            opts.uia_shallow_materialize_budget_ms = 10;
        }
        "explorer.exe" => {
            // Win32-first path may under-list shell content; enrich below threshold.
            opts.explorer_enrich_if_below = 8;
            opts.explorer_enrich_materialize_budget_ms = 28;
        }
        _ => {}
    }
}
