# M9 — Fallbacks (acceptance)

This document ties together **Milestone 9** (`10-milestones.md`) and the shipped code.

## Implemented in-tree

| Piece | Location |
|-------|----------|
| UIA → MSAA → raw-HWND ladder | `crates/nav-uia/src/runtime.rs` (`FallbackPolicy::Auto`) |
| MSAA (`IAccessible`) | `crates/nav-uia/src/fallback_msaa.rs` |
| Raw `EnumChildWindows` | `crates/nav-uia/src/fallback_hwnd.rs` |
| Soft stage budgets (defaults) | `crates/nav-uia/src/options.rs` (`M9_DEFAULT_BUDGET_*_MS`) |
| Config defaults | `crates/nav-config/src/lib.rs` `[fallback.budget_ms]` — must match `M9_DEFAULT_*` |
| Invoke by `Backend` | `crates/nav-uia/src/runtime.rs` `invoke`, `invoke_msaa_at`, `left_click_rect_center` |
| Tray “Diagnose” (UIA dump) | `crates/nav-uia/src/diagnose.rs` + `nav-app` tray handler |

## Automated checks (CI)

`cargo test -p nav-uia` on **Windows** runs `tests/m9_acceptance.rs`:

- `UiaRuntime::new` (COM + UIAutomation).
- `enumerate` on a live **`Shell_TrayWnd`** for `Auto` / `UiaOnly` / `MsaaOnly` (no panic / hard error).
- **Repeat** enumeration twice with the same options → same hint count (sanity for stable roots).
- **~80** sequential `Auto` enumerations on the same HWND (repeatability; count in `tests/m9_acceptance.rs`; not the full 1000-trigger field gate — see below).

## Coverage matrix (Win32 / WPF / …)

Quantitative per-app **coverage** targets in `00-overview.md` (e.g. 95% Win32) are **not** run as automated % in CI — that remains a **manual** / fixture-machine gate.

## 1000-trigger / 99.9% field gate

The milestone text’s **1000-trigger reliability** and **99.9%** success on a real fixture set is a **release / dogfood** check, not wired into GitHub Actions (no headless `navigator` loop with real apps in the default PR workflow).

**Suggested manual run:** on a dev box, drive hint sessions against the reference fixtures (e.g. Run dialog, Notepad, MFC test apps) in a script or AHK loop; file issues if the rate falls below the bar.

## Changing default budgets

1. Update `M9_DEFAULT_BUDGET_*_MS` in `nav-uia` `options.rs`.
2. Update the `default_budget_*` functions in `nav-config` to the same numbers.
3. Re-run `cargo test -p nav-uia` and `cargo test -p nav-config` (and refresh `default_config_toml` expectations if the serialized TOML changes).
