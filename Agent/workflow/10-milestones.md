# 10 — Milestones

> Prototype → Standard → Elite. Twelve milestones, each with a single
> demonstrable deliverable and a hard exit gate. **Do not skip ahead.** A
> milestone is "done" only when its checks are green.

Each milestone lists:

- **Theme** — the one-line goal.
- **Scope** — what we add.
- **Demo** — the runnable artifact at the end.
- **Gate** — the measurable criterion.
- **Anti-scope** — what we explicitly do not do here.

Time estimates assume one focused engineer. Pad ×1.5 for first-time-on-Windows
contributors.

---

## M0 — Foundations (½ day)

**Theme:** Repo is a Cargo workspace and CI is green on an empty build.

**Scope:**
- `Cargo.toml` workspace, `rust-toolchain.toml`, `.cargo/config.toml`,
  `rustfmt.toml`, `clippy.toml`, `deny.toml`.
- New `.gitignore` (Rust-shaped). Old `.gitignore` archived under
  `legacy/`.
- `.github/workflows/ci.yml`: fmt + clippy + `cargo deny check` + tests on
  Linux + Windows runners.
- `legacy/` exists; old `src/` moved verbatim (see `11-legacy-migration.md`).

**Demo:** PR template merge runs CI green on both runners. `cargo build
--workspace` is a no-op (no crates yet).

**Gate:** CI green. Single PowerShell command `cargo check --workspace`
returns exit 0 in < 5 s on a stock laptop.

**Anti-scope:** any new code. M0 is plumbing only.

---

## M1 — Pure-Rust core (1 day)

**Theme:** The label, filter, and session logic — testable on Linux.

**Scope:**
- `nav-core` crate. `geom`, `hint`, `label`, `filter`, `session`, `error`.
- Unit tests covering legacy HAP cases and edge cases.
- Proptest invariants on label generator (prefix-free, alphabet-only).
- Set up `nav-bench` skeleton with `cargo bench --bench label` placeholder.

**Demo:** `cargo test -p nav-core` and `cargo bench -p nav-bench --bench
label` on Linux CI.

**Gate:**
- 25+ tests pass.
- `bench label`: ≤ 250 ns / 1024 hints P95.
- Linux CI build proves we have no Windows leakage.

**Anti-scope:** `windows` crate, COM, rendering.

---

## M2 — Windows hello + hotkey (1 day)

**Theme:** Press `Alt+;`, see a log line. End-to-end Windows wiring.

**Scope:**
- `nav-app` minimum viable binary. Single-instance lock. Manifest with
  `PerMonitorV2` + `asInvoker`. Versioned exe resource.
- `nav-input::hotkey` with `RegisterHotKey` and a message-only window.
- `tracing`-based console logger gated by `--log <level>`.

**Demo:** Run the binary, press `Alt+;`, see `[input] hotkey id=0
captured_hwnd=...` in the console.

**Gate:**
- Hotkey latency (WM_HOTKEY → handler tx_send) ≤ 1 ms P95 over 1000
  presses.
- Second instance exits cleanly with code 2.
- Windows CI builds and starts the binary in headless mode for a smoke
  test (process exits within 1 s on `--smoke`).

**Anti-scope:** UIA, rendering, hint logic. We just route the hotkey.

---

## M3 — UIA enumeration baseline (2 days)

**Theme:** Real elements on real apps, with the *unoptimized* COM path.

**Scope:**
- `nav-uia::runtime`: COM init, `IUIAutomation` instantiation.
- `nav-uia::enumerate`: `FindAll(TreeScope_Descendants, ...)`-based
  enumeration. Per-element pattern and rect calls.
- `nav-uia::pattern`: invoke dispatch (Invoke, Toggle, Selection,
  ExpandCollapse).

**Demo:** Hotkey on Notepad logs `[uia] enum hwnd=0x... elements=12
took=83ms`. The numbers will be ugly. That's the point.

**Gate:**
- Enumerates Notepad, File Explorer, VS Code without crashing.
- Element count is non-zero and matches the legacy HAP within ±5%.
- Baseline numbers recorded in `12-benchmarking.md` so we can prove the
  Phase D wins.

**Anti-scope:** caching, parallelism, fallbacks. Yes, this will be slow.

---

## M4 — First overlay (2 days)

**Theme:** A click-through, top-most layered window with hardcoded hints.

**Scope:**
- `nav-render::overlay` + `device` + `d2d` + `monitors`.
- A single overlay on the primary monitor for now.
- Direct2D + DirectComposition pipeline. Five hardcoded "hint pills".
- Render thread with `crossbeam::channel`-driven loop.

**Demo:** Hotkey shows five rounded-rect pills with letters at fixed
screen positions. Esc dismisses.

**Gate:**
- Click-through verified: clicks under the overlay reach the underlying
  app.
- No flicker on show/hide. Overlay never gets focus
  (`GetForegroundWindow` does not return our HWND).
- Frame budget: render thread `update()` ≤ 4 ms P95.

**Anti-scope:** real hints from UIA, multiple monitors, glyph atlas.

---

## M5 — End-to-end MVP (2 days)

**Theme:** Hotkey → enumerate → render → type → invoke. Ugly but real.

**Scope:**
- Wire `nav-uia` results through `nav-core::planner` to `nav-render`.
- `nav-input::ll_hook` for hint-mode key capture.
- Orchestrator state machine (`nav-app::orchestrator`).
- `Esc` cancels; foreground change cancels; re-press cancels.

**Demo:** Open Notepad. `Alt+;`. Hint pills appear over `File`, `Edit`,
`View`, `Help`, the close button. Type the label. The menu opens.

**Gate:**
- 100/100 trigger-to-invoke success on the demo path.
- No stuck overlays after 100 cycles.
- Orchestrator panics under fault injection are recovered cleanly (LL
  hook is removed, overlay is hidden).

**Anti-scope:** performance optimization, fallbacks, multi-monitor,
configuration.

> 🟢 **MVP complete.** Navigator is functionally a v0. Now we make it fast.

---

## M6 — UIA cache request (1 day)

**Theme:** The single biggest perf win.

**Scope:**
- Build `IUIAutomationCacheRequest` once at boot with all properties &
  patterns we read. Use `AutomationElementMode_None`.
- Switch enumeration to `BuildUpdatedCache` + cached TreeWalker.
- Remove all per-element `GetCurrentPattern`/`Current*Property` calls.

**Demo:** Run `nav-bench enumerate_real`. Reference numbers drop
dramatically vs M3 baseline.

**Gate:**
- Notepad enumeration P95 ≤ 6 ms.
- File Explorer P95 ≤ 15 ms.
- VS Code P95 ≤ 22 ms.
- Element coverage **unchanged** vs M3 (compare element-id sets).

**Anti-scope:** parallelism, fallbacks.

---

## M7 — Pre-warm (1 day)

**Theme:** First hotkey is as fast as the thousandth.

**Scope:**
- COM init in workers at boot.
- D3D/D2D devices created at boot.
- Per-monitor overlay windows created hidden at boot.
- Brushes, default text format, scratch buffers preallocated.
- Smoke test: start binary, immediately press hotkey via `keybd_event`,
  measure first-trigger latency.

**Demo:** `cold_start_latency.ps1` reports < 30 ms on the first hotkey
after a fresh boot.

**Gate:**
- First-session latency ≤ warm latency + 5 ms P95.
- Cold-start to ready ≤ 150 ms.

**Anti-scope:** anything else.

---

## M8 — Multi-monitor + DPI (2 days)

**Theme:** Correct on the realistic dev setup.

**Scope:**
- Per-monitor overlay windows.
- `WM_DISPLAYCHANGE` / `WM_DPICHANGED_AFTERPARENT` rebuild flow.
- Hint-to-monitor assignment by center point.

**Demo:** 4K + 1080p side-by-side. Drag a window across monitors,
hotkey works on each, hints appear at correct sizes.

**Gate:**
- Verified on 100% / 125% / 150% / 175% / 200% scaling.
- No flicker when DPI changes mid-session.

**Anti-scope:** glyph atlas, fallbacks.

---

## M9 — Fallbacks (3 days)

**Theme:** Reliability beats coverage.

**Scope:**
- `nav-uia::fallback_msaa` — `IAccessible` enumerator.
- `nav-uia::fallback_hwnd` — `EnumChildWindows` walker + `SendInput`
  click.
- Orchestrator fallback ladder with per-step time budgets (25 / 8 / 5 ms).
- "Diagnose" tray menu item that captures a UIA dump for failing windows.

**Demo:** Win+R "Run" dialog gets hints (UIA may return 0 here on some
builds; MSAA covers it). An older MFC test fixture also gets hints.

**Gate:**
- Element coverage by app type meets the matrix in `00-overview.md`.
- 1000-trigger reliability test on a fixture set: 99.9% success.

**Anti-scope:** glyph atlas, progressive reveal.

---

## M10 — Configuration + tray (2 days)

**Theme:** Users can change the things they care about.

**Scope:**
- `nav-config` full schema: hotkey, alphabet, font size, colors,
  exclusions, log level.
- CLI: `--config <path>`, `--print-config`, `--reset-config`.
- Tray icon with `Reload config`, `Open config folder`, `About`, `Quit`.
- Hot reload of brushes / font / hotkey without restart.

**Demo:** Change `font_size_px` in the config, click `Reload`, next
session uses the new size without any other restart.

**Gate:**
- Config round-trip test (load → serialize → load → equal).
- Reload flow does not crash, does not leak GPU resources, does not
  miss hotkeys.

**Anti-scope:** GUI settings panel. The config file *is* the settings.

---

## M11 — Release engineering (2 days)

**Theme:** Shipping the binary.

**Scope:**
- `release.yml` GitHub Actions workflow: build, sign, package.
- Three artifacts: signed `.exe`, portable `.zip`, `.msix` installer.
- Authenticode signing via repo secret.
- Release notes template tied to milestones.
- Auto-update check (M11 stretch — *off by default*; users opt in).

**Demo:** Tag `v0.1.0`, the workflow produces three downloadable
artifacts on the GitHub Release page.

**Gate:**
- The `.exe` runs on a fresh Win10 22H2 VM and a fresh Win11 24H2 VM.
- SmartScreen passes after a few hundred installs (Microsoft reputation
  is earned, not bought).

**Anti-scope:** auto-update mechanism, telemetry. Stay opt-in if at all.

> 🟢 **v1.0 shipped.** Standard target met (P95 ≤ 30 ms, all gates green).

---

## Post-v1 milestones (Elite)

### M12 — Glyph atlas (2 days)

**Theme:** Render submit ≤ 0.8 ms P95.

Pre-rasterize the configured alphabet at the configured size into a single
`ID2D1Bitmap1`. Render hints as instanced quads sampling the atlas.

Gate: P95 render submit < 0.8 ms; visual diff against `DrawText` path is
indistinguishable.

### M13 — Progressive reveal (2 days)

**Theme:** No app *ever* feels frozen.

For trees > 256 elements, emit chunks every ~4 ms; render shows immediately.

Gate: On a 5000-element synthetic tree, first hint visible at ≤ 8 ms,
last hint visible at ≤ 30 ms.

### M14 — ETW + flame tooling (1 day)

**Theme:** Profileable in the wild.

Custom ETW provider with structured events. `tools/flame.ps1` renders a
flame graph.

Gate: `nav-bench enumerate_real` is profiled and the flame graph is
attached to the perf section of the README.

### M15 — Stretch features (off by default)

Only after v1 is rock-solid:

- Taskbar hint mode.
- Click-mode (hold a modifier to *click* without invoking).
- AutoHotkey integration sample.
- Optional autostart entry.

Each gated behind a config flag, defaulting off.

---

## Out of scope, period

- Themes, animations, plugins, macros, AI, cloud sync, telemetry.
- Mobile / web / cross-platform.
- "Smart" predictive hints.
- Settings GUI.

A request for any of these is a request for a different product.

---

## Status board (live)

Engineers update this table as part of each PR.

| ID  | Milestone                  | Status   | P95 metric           | Notes |
|-----|----------------------------|----------|----------------------|-------|
| M0  | Foundations                | TODO     | —                    |       |
| M1  | Pure-Rust core             | TODO     | —                    |       |
| M2  | Windows hello + hotkey     | TODO     | —                    |       |
| M3  | UIA baseline               | TODO     | —                    |       |
| M4  | First overlay              | TODO     | —                    |       |
| M5  | End-to-end MVP             | TODO     | —                    |       |
| M6  | UIA cache                  | TODO     | —                    |       |
| M7  | Pre-warm                   | TODO     | —                    |       |
| M8  | Multi-monitor              | TODO     | —                    |       |
| M9  | Fallbacks                  | TODO     | —                    |       |
| M10 | Config + tray              | TODO     | —                    |       |
| M11 | Release                    | TODO     | —                    |       |
| M12 | Glyph atlas (post-v1)      | TODO     | —                    |       |
| M13 | Progressive reveal         | TODO     | —                    |       |
| M14 | ETW + flame                | TODO     | —                    |       |
| M15 | Stretch features           | TODO     | —                    |       |
