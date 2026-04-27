# 13 — Configuration

> Configuration is a tax on the user and the maintainer. Every option is a
> decision the user must make and a code path we must keep working.
> **Bias hard toward sensible defaults.**

**Repo state:** `crates/nav-config` + **`navigator`** use **`load_for_startup`**
(see discovery order below). Implemented sections: **`[hints]`**, **`[log]`**
(optional `level`, merged with `--log` at startup), **`[fallback.budget_ms]`**
(`uia` / `msaa` / `hwnd`, defaults **25 / 8 / 5**). CLI: **`--config`**, **`--print-config`**,
**`--reset-config`**, **`--no-tray`**. Tray **Reload** re-reads config (hints +
budgets). **Not** yet: full schema below, automatic discovery writes,
`--edit-config`, **`[appearance]`** reload into DWrite, or hotkey chord from file.

## Principles

1. **Single file**, TOML, human-readable.
2. **Zero options visible by default.** The default config is empty; users
   add only what they want to change.
3. **CLI flags override file** for ad-hoc experimentation.
4. **Never silent reinterpretation.** If the user typed an invalid value,
   we tell them — we don't guess.
5. **Hot reload** for everything except the hotkey (which we deliberately
   re-register cleanly).

## Discovery order

The first found wins:

1. `--config <path>` CLI flag.
2. Environment variable `NAVIGATOR_CONFIG`.
3. `%APPDATA%\Navigator\config.toml` (per-user, default for installed
   builds).
4. `<exe-dir>\config.toml` (portable / dev).
5. Built-in defaults (compiled in via `include_str!`).

The merged effective config is what `nav-config::load` returns. CLI flags
that match config keys (`--hotkey`, `--alphabet`, `--font-size`, etc.)
override the merged file at the very end.

## File schema (`config.toml`)

The full schema, with every supported key. Every key is **optional**;
omitting it uses the default.

```toml
# config.toml — Navigator configuration
# Anything you don't set falls back to the built-in default.
# Run `nav.exe --print-config` to see the merged effective values.

[hotkey]
# Modifier list and a key. Modifiers: ctrl, alt, shift, win.
# Key: any single char (case-insensitive) or a named key (esc, tab, f1..f12,
# semicolon, slash, ...).
# Default: alt+;
chord = "alt+;"

# Optional secondary chord for taskbar mode (post-v1).
# Default: ctrl+;
taskbar_chord = "ctrl+;"

# Dev-only chord for printing the UIA tree dump.
# Default: alt+shift+;
debug_chord = "alt+shift+;"


[hints]
# Characters used for labels, in priority order.
# First chars get short labels, later chars are appended for long labels.
# Default: "sadfjkle wcmpgh"  (14 home-row-first chars)
alphabet = "sadfjkle wcmpgh"

# Maximum elements to label per session. Hard cap.
# Default in code today: 2048 (see nav-config / nav-uia EnumOptions).
max_elements = 2048

# When the user types a non-alphabet character: "ignore" or "cancel".
# Default: "cancel"
on_unknown_key = "cancel"


[appearance]
# Pill background color, RGBA hex.   Default: "1B1F2EE6"
bg = "1B1F2EE6"
# Pill border color, RGBA hex.       Default: "3D7DFF80"
border = "3D7DFF80"
# Foreground color (typed prefix).   Default: "3D7DFF"
fg_typed = "3D7DFF"
# Foreground color (remaining char). Default: "FFFFFF"
fg_rest = "FFFFFF"
# Foreground color (filtered out).   Default: "5C677A"
fg_dim = "5C677A"
# Pill corner radius in pixels.      Default: 4
radius_px = 4
# Padding around the label in px.    Default: 3
padding_px = 3
# DirectWrite font family.           Default: "Segoe UI Variable Display"
font_family = "Segoe UI Variable Display"
# DirectWrite weight (100..900).     Default: 600
font_weight = 600
# Font size in px.                   Default: 14
font_size_px = 14


[behavior]
# How the orchestrator handles a session timeout (no key press).
# Set to 0 to disable timeout (default).
# Default: 0  (off)
timeout_ms = 0

# When the foreground window changes mid-session, cancel?
# Default: true
cancel_on_focus_change = true

# Re-pressing the hotkey while in hint mode cancels the session.
# Default: true
cancel_on_repress = true


[exclusions]
# Window class names to never hint. Glob patterns supported.
classes = [
  "Shell_TrayWnd",
  "Windows.UI.Core.CoreWindow",
  "MultitaskingViewFrame",
]
# Process names (case-insensitive, exe basename) to never hint.
processes = []


[fallback]
# Order of enumerators tried for each session.
# Allowed values: ["uia", "msaa", "hwnd"]. Order matters.
# Default: ["uia", "msaa", "hwnd"]
order = ["uia", "msaa", "hwnd"]
# Per-step time budget in milliseconds.
# Default: { uia = 25, msaa = 8, hwnd = 5 }
budget_ms = { uia = 25, msaa = 8, hwnd = 5 }


[log]
# trace | debug | info | warn | error | off
# Default: warn
level = "warn"
# Path. Empty = log to console only. Defaults: empty.
file = ""
```

That entire file is documented inline. The default config file we ship
under `assets/default-config.toml` is **shorter** — just commented-out
examples — because the real defaults live in code. Users only set what
they want different.

### Built-in default file (`assets/default-config.toml`)

```toml
# Navigator default config.
# Uncomment and edit any line to override.
# Run `nav.exe --print-config` to see the merged effective values.

# [hotkey]
# chord = "alt+;"

# [hints]
# alphabet = "sadfjkle wcmpgh"

# [appearance]
# font_size_px = 14

# [exclusions]
# processes = ["secret-app.exe"]
```

## Schema in Rust

```rust
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub hotkey: HotkeySection,
    pub hints: HintsSection,
    pub appearance: AppearanceSection,
    pub behavior: BehaviorSection,
    pub exclusions: ExclusionsSection,
    pub fallback: FallbackSection,
    pub log: LogSection,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct HotkeySection {
    pub chord: ChordSpec,
    pub taskbar_chord: ChordSpec,
    pub debug_chord: ChordSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChordSpec {
    /// Bitmask: 0x1 ctrl, 0x2 alt, 0x4 shift, 0x8 win.
    pub modifiers: u8,
    /// Win32 VK code resolved at parse time, layout-independent.
    pub vk: u32,
    /// Original spec string for diagnostics.
    pub raw: String,
}
```

Implement `serde::Deserialize` for `ChordSpec` to parse `"alt+;"`, return
a structured error on invalid input (`unknown modifier "ctl"`, `unknown key
"semicolan"`, etc.).

## Validation

`load()` performs the following validation **after** merge:

1. `alphabet.chars().count() ≥ 2`. Single-char alphabet is meaningless.
2. `alphabet` contains no whitespace, no duplicates, all printable ASCII.
3. `max_elements ∈ [1, 8192]`.
4. Color hex strings: 6 or 8 hex chars after a leading optional `#`.
5. `font_weight ∈ {100, 200, …, 900}`.
6. `font_size_px ∈ [6, 96]`.
7. `fallback.order` is a non-empty subset of `["uia","msaa","hwnd"]` with
   no duplicates.
8. `timeout_ms ∈ [0, 600_000]`.

Any violation returns `ConfigError` with a precise location (file path +
key path). The orchestrator surfaces this to the tray balloon and refuses
to start with the bad config (falling back to the embedded defaults so the
hotkey at least keeps working).

## CLI surface

`clap` derive types in `nav-config::cli`:

```rust
#[derive(clap::Parser, Debug)]
#[command(name = "nav", version, about = "Keyboard hint navigator for Windows")]
pub struct CliArgs {
    /// Path to a config.toml. Overrides discovery.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Hotkey override (e.g. "ctrl+alt+;").
    #[arg(long)]
    pub hotkey: Option<String>,

    /// Alphabet override.
    #[arg(long)]
    pub alphabet: Option<String>,

    /// Font size override in px.
    #[arg(long)]
    pub font_size: Option<u32>,

    /// Log level: trace | debug | info | warn | error | off.
    #[arg(long)]
    pub log: Option<String>,

    /// Print the effective config and exit.
    #[arg(long)]
    pub print_config: bool,

    /// Reset the config file at the discovered location to defaults and exit.
    #[arg(long)]
    pub reset_config: bool,

    /// Open the config file in the default editor and exit.
    #[arg(long)]
    pub edit_config: bool,

    /// Synthetic smoke test: start, fire hotkey once, exit. CI-only.
    #[arg(long, hide = true)]
    pub smoke: bool,

    /// Trigger one hint session immediately (legacy HAP parity).
    #[arg(long)]
    pub hint: bool,

    /// Start in tray-only mode (legacy HAP parity).
    #[arg(long)]
    pub tray: bool,
}
```

`--hint` and `--tray` exist for AutoHotkey users who already script HAP
that way. Same syntax, same semantics — keeps muscle memory.

## Hot reload

The tray menu has `Reload config`. The orchestrator on click:

1. Calls `nav-config::load(&args)`. If it fails, surface the error and
   keep the current config.
2. Diffs old vs new. For each section that changed:
   - `hotkey.*` → unregister + re-register `RegisterHotKey`.
   - `hints.*` → next session uses new alphabet/cap.
   - `appearance.*` → drop `IDWriteTextFormat` and brushes; recreate.
   - `exclusions.*` → next session uses new lists.
   - `log.*` → reload tracing subscriber filter.

The render thread is paused for the duration of `appearance` reload.
Total reload latency target: ≤ 30 ms.

## File watcher (M10 stretch)

Optional: watch `config.toml` with `notify` and auto-reload. Off by
default. Enabled with `--watch-config`. Useful for theme tweaking.

## AutoHotkey integration

Users who run AHK scripts can drive Navigator without a hotkey by spawning
the binary with `--hint` (single-shot) or `--tray` (background). Same as
legacy HAP for muscle memory.

Example AHK:

```ahk
; Single-shot hint mode bound to Win+;
#`;::Run, "C:\Program Files\Navigator\nav.exe" --hint
```

The `--hint` mode launches, fires one session, exits when the user types
a label or presses Esc. Cold-start matters here: M7's pre-warm budget
keeps `--hint` ≤ 200 ms total.

## Anti-features (do not add)

- ❌ JSON config format (TOML is enough).
- ❌ A YAML config format (no).
- ❌ Multiple "profiles" with switch-by-app rules (v1).
- ❌ A settings GUI app (v1+ never).
- ❌ Cloud sync (never).

If a user wants per-app config, they can write a tiny launcher script
that swaps `--config` paths.
