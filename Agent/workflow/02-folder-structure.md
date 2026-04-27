# 02 — Folder Structure

## Top-level repository layout

```
navigator/
├── Agent/                          # Project workflow (this folder lives here)
│   └── workflow/                   # Read README.md first
├── crates/                         # All Rust crates live here
│   ├── nav-core/                   # Pure logic. No Win32. Cross-platform testable.
│   ├── nav-config/                 # TOML config types + `load()` ([hints] subset today).
│   ├── nav-uia/                    # UI Automation enumerator + invoker.
│   ├── nav-input/                  # Hotkey + low-level keyboard hook.
│   ├── nav-render/                 # Layered window + Direct2D/DComp overlay.
│   ├── nav-app/                    # The shipping binary.
│   └── nav-bench/                  # Criterion benches and synthetic harnesses.
├── assets/                         # Static, non-code files shipped with the binary.
│   ├── icon.ico                    # Tray + window icon.
│   └── default-config.toml         # Default config, embedded via include_str!.
├── tools/                          # Dev-only utilities (Rust or PowerShell).
│   ├── bench-runner.ps1            # Wrapper that drives nav-bench against real apps.
│   ├── flame.ps1                   # Captures an ETW + flamegraph.
│   └── trace-uia.ps1               # WPR profile for UIA-heavy enumeration.
├── tests/                          # Cross-crate integration tests.
│   ├── e2e/                        # End-to-end against fixture HWNDs.
│   └── fixtures/                   # Tiny apps used as reference targets.
├── docs/                           # Public-facing docs (release notes, install guide).
├── legacy/                         # Old C# Hunt-and-Peck. Read-only reference.
│   ├── README.md                   # Pointer note explaining what this is.
│   └── src/                        # Original src/ moved here verbatim.
├── screenshots/                    # Marketing/README assets.
├── .cargo/
│   └── config.toml                 # Workspace-wide rustflags + LTO config.
├── .github/
│   └── workflows/
│       ├── ci.yml                  # Build + test + clippy + fmt + bench-smoke.
│       └── release.yml             # Tagged release: signed binary, MSIX, zip.
├── Cargo.toml                      # Workspace root (virtual manifest).
├── Cargo.lock                      # Committed.
├── deny.toml                       # cargo-deny: license + advisory + bans + sources.
├── rustfmt.toml                    # Project formatter rules.
├── clippy.toml                     # Clippy lint config.
├── rust-toolchain.toml             # Pinned toolchain (stable, MSRV explicit).
├── LICENSE
└── README.md                       # User-facing readme (replaces old HAP readme).
```

### Why this layout

- **`crates/` not flat.** A workspace keeps build artifacts shared (`target/`),
  enables `cargo test --workspace`, and lets us lint dependencies per-crate.
- **`Agent/workflow/` not in `docs/`.** Workflow is for builders. `docs/` is for
  users. They have different audiences and different stability guarantees.
- **`legacy/` at top level.** Easy to find. Easy to nuke when no longer useful.
- **`tools/` PowerShell-friendly.** Windows-first project; `.ps1` is fine.
- **`assets/` separate from `nav-app/`.** Lets `nav-bench` reuse the same icon
  without weird path tricks.

## Cargo workspace root (`Cargo.toml`)

```toml
[workspace]
resolver = "2"
members = [
    "crates/nav-core",
    "crates/nav-config",
    "crates/nav-uia",
    "crates/nav-input",
    "crates/nav-render",
    "crates/nav-app",
    "crates/nav-bench",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
repository = "https://github.com/<owner>/navigator"

[workspace.dependencies]
# Internal
nav-core   = { path = "crates/nav-core" }
nav-config = { path = "crates/nav-config" }
nav-uia    = { path = "crates/nav-uia" }
nav-input  = { path = "crates/nav-input" }
nav-render = { path = "crates/nav-render" }

# External — versions pinned via Cargo.lock; bump deliberately.
windows           = "0.59"
windows-core      = "0.59"
parking_lot       = "0.12"
crossbeam-channel = "0.5"
rayon             = "1.10"
smallvec          = "1.13"
ahash             = "0.8"
serde             = { version = "1", features = ["derive"] }
toml              = "0.8"
clap              = { version = "4", features = ["derive"] }
tracing           = "0.1"
tracing-subscriber = "0.3"
thiserror         = "2"
once_cell         = "1.20"

# Bench-only
criterion = "0.5"
```

### Workspace-wide release profile

`.cargo/config.toml`:

```toml
[build]
rustflags = ["-C", "target-cpu=x86-64-v2"]   # Win10/11 floor; SSE4.2 minimum.

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"
debug = false
incremental = false

[profile.release-with-debug]
inherits = "release"
debug = "line-tables-only"
strip = "none"
```

Rationale:

- `target-cpu=x86-64-v2` is the modern Windows floor (Win11 already requires
  it). Buys us SSE4.2 which the hint label generator and string compare paths
  benefit from.
- `lto = "fat"` shaves a measurable ~6% off enumeration hot path. Acceptable
  build-time cost for a release-only binary.
- `panic = "abort"` shrinks the binary by ~120 KB and kills the unwinding
  cost in COM callbacks. We do not need unwinding; an unrecoverable error
  exits and the user re-triggers.
- `release-with-debug` is the profile you ship to a profiler.

## Per-crate skeletons

### `crates/nav-core`

```
nav-core/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── hint.rs                # Hint, RawHint, HintId, ElementKind.
    ├── label.rs               # Vimium-style label generator (pure fn).
    ├── filter.rs              # Prefix matching, candidate set update.
    ├── planner.rs             # Layout-aware ranking, deduplication.
    ├── session.rs             # Session<T>, state machine.
    ├── geom.rs                # Rect, Point, scale-independent math.
    ├── error.rs               # NavError enum.
    └── tests/
        ├── label_tests.rs
        ├── filter_tests.rs
        └── planner_tests.rs
```

**Rule:** `nav-core` compiles on Linux (run `cargo test -p nav-core` locally).
CI today is **Windows-only** and runs `cargo test --workspace`. If a contributor
adds a Win32 type under `nav-core`, that is still incorrect — keep this crate OS-agnostic.

### `crates/nav-config`

```
nav-config/
├── Cargo.toml
└── src/
    └── lib.rs                 # Config / HintsConfig, `load`, `alphabet_chars` (expand toward schema.rs, discovery, etc.).
```

### `crates/nav-uia`

```
nav-uia/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── runtime.rs             # COM init, IUIAutomation singleton, cache request build.
    ├── enumerate.rs           # Cached enumeration via TreeWalker + BuildUpdatedCache.
    ├── pattern.rs             # Invoke / Toggle / Select / ExpandCollapse / Value.
    ├── coords.rs              # DPI-aware bounding rect → window-local rect.
    ├── fallback_msaa.rs       # IAccessible enumerator (Phase E).
    ├── fallback_hwnd.rs       # Raw EnumChildWindows walker (Phase E).
    └── invoke.rs              # Execute action chosen by pattern dispatch.
```

### `crates/nav-input`

```
nav-input/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── hotkey.rs              # RegisterHotKey + message-only window.
    ├── ll_hook.rs             # WH_KEYBOARD_LL during hint mode.
    ├── keymap.rs              # VK code → hint-alphabet char (layout-aware).
    └── thread.rs              # Owns the input thread + its message pump.
```

### `crates/nav-render`

```
nav-render/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── overlay.rs             # Layered window per monitor; lifecycle.
    ├── device.rs              # D3D11 + DXGI + DComp device cache.
    ├── d2d.rs                 # Direct2D context, brushes, text format.
    ├── glyph_atlas.rs         # Pre-rasterized hint glyphs (M9 elite path).
    ├── scene.rs               # Scene = list of HintQuad; diff + redraw.
    └── monitors.rs            # EnumDisplayMonitors, per-monitor DPI awareness.
```

### `crates/nav-app`

```
nav-app/
├── Cargo.toml
├── build.rs                   # Embeds icon, manifest, version resource.
├── app.manifest               # Per-monitor V2, supportedOS for Win10+.
└── src/
    ├── main.rs                # Entry point; session loop + UIA/render wiring (orchestrator lives here for now).
    ├── single_instance.rs     # Named-mutex guard.
    └── logging.rs             # tracing subscriber.
```
*(Tray module and split `orchestrator.rs` are roadmap — see `10-milestones.md` M10.)*

### `crates/nav-bench`

```
nav-bench/
├── Cargo.toml
└── benches/
    ├── label.rs               # Criterion: label generation throughput.
    ├── filter.rs              # Criterion: prefix filter throughput.
    ├── planner.rs             # Criterion: planner + labels.
    ├── session.rs             # Criterion: Session::key hot path.
    └── enumerate_synth.rs     # dedupe + plan on synthetic RawHints (no COM).
```

`enumerate_real` (fixture Win32 apps) is not in-tree yet. **Level 1** benches are
pure Rust and can run on any host; **CI** runs `cargo bench -p nav-bench -- --quick`
on Windows.

## Files explicitly removed from `legacy/` migration

Once HAP is moved into `legacy/`, the following are deleted from the new tree:

- `src/.nuget/`, `src/packages/`, `src/tools/` (NuGet / Cake / build-tooling
  remnants — not relevant to a Rust toolchain).
- `src/build.cake`, `src/build.ps1` (replaced by Cargo + GitHub Actions).
- `src/HuntAndPeck.sln`, `*.csproj` (kept *inside* `legacy/` for reference).

The contents of `legacy/` are **never** modified by Navigator development.
They exist solely as reference. CI does not build them.

## File naming and casing

- Rust files: `snake_case.rs`. No exceptions.
- Markdown: `kebab-case.md` for docs, `NN-name.md` (zero-padded) for ordered
  workflow.
- PowerShell: `kebab-case.ps1`.
- Configuration: `lowercase.toml` (e.g. `config.toml`, `deny.toml`).

## What goes in version control

- All source, all configs, `Cargo.lock`, the manifest, the icon, embedded
  default config.
- `.cursor/`, `.git/`, `.vs/`, `target/`, `out/`, `dist/` — never committed.

The `.gitignore` we inherit from the legacy HAP needs a Rust-aware rewrite
during M0; see `04-build-order.md`.
