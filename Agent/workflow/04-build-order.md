# 04 — Build Order

> Strict dependency order. Do not skip ahead. Each step ends with a runnable
> deliverable and a green check. If a step's checks fail, stop and fix them
> before the next step.

This is the **daily checklist**. Each step lists:

- **Goal** — the single thing this step accomplishes.
- **Touches** — which crates / files.
- **Done when** — the runnable check that proves it.
- **Anti-goals** — what *not* to add yet.

---

## Phase A — Foundations (no Win32 yet)

### A1. Repo skeleton & toolchain pin

- **Goal:** create the Cargo workspace and pin the toolchain.
- **Touches:** `Cargo.toml` (workspace), `rust-toolchain.toml`,
  `.cargo/config.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`,
  `.gitignore`, `.github/workflows/ci.yml`.
- **Done when:**
  - `cargo --version` matches the pinned toolchain.
  - `cargo check --workspace` succeeds (empty crates only).
  - CI runs `fmt`, `clippy -- -D warnings`, `cargo deny check` on PR.
- **Anti-goals:** no `windows` crate yet; no business logic.

### A2. `nav-core::geom` + `nav-core::label`

- **Goal:** ship the pure-Rust label generator with full unit tests.
- **Touches:** `crates/nav-core/src/{lib.rs, geom.rs, label.rs}` and tests.
- **Done when:**
  - `cargo test -p nav-core` passes (≥ 25 label tests covering edge cases:
    0 elements, 1, 14, 15, 196, 197, 1000, alphabet length 2, etc.).
  - Property test asserts labels are **prefix-free** for n ∈ [0, 5000].
  - `cargo bench -p nav-bench --bench label` finishes; record the P50/P95
    in `12-benchmarking.md`.
- **Anti-goals:** no rendering, no COM, no `Hint` yet.

### A3. `nav-core::filter` + `nav-core::session`

- **Goal:** ship the prefix filter and the session state machine.
- **Touches:** `crates/nav-core/src/{filter.rs, session.rs, hint.rs}`.
- **Done when:**
  - State-machine table tests cover all transitions in
    `01-architecture.md`.
  - Fuzz test (cargo-fuzz or proptest) of 100k random keystrokes never
    leaves session in an invalid state and never panics.
- **Anti-goals:** no Win32 integration.

### A4. `nav-config` schema + loader

- **Goal:** parse `config.toml`, with defaults embedded.
- **Touches:** `crates/nav-config/**`, `assets/default-config.toml`.
- **Done when:**
  - Round-trip test: load defaults, serialize, parse, equals.
  - CLI `--config` flag overrides; `--print-config` dumps merged config.
  - Discovery order test (env > CLI > APPDATA > exe-dir > defaults).

**Phase A exit criterion:** Linux CI (`ubuntu-latest`) builds and tests the
entire workspace except `nav-app`, `nav-input`, `nav-uia`, `nav-render`,
`nav-bench/enumerate_real`. We have a portable, testable core. **No Windows
APIs touched yet.**

---

## Phase B — Windows hello

### B1. `nav-app` shell with single-instance lock

- **Goal:** a Windows binary that prints "Navigator ready" and exits cleanly,
  with a named-mutex preventing double launches.
- **Touches:** `crates/nav-app/src/{main.rs, single_instance.rs, logging.rs}`,
  `crates/nav-app/build.rs`, `crates/nav-app/app.manifest`.
- **Done when:**
  - Manifest declares `PerMonitorV2` and `requestedExecutionLevel asInvoker`.
  - Second launch exits with code 2 and brings the running instance to the
    foreground (via `BringWindowToTop` on the message-only window).
  - `tracing` is wired to a console subscriber gated by
    `--log <level>`.

### B2. `nav-input::hotkey` — Alt+; round-trip

- **Goal:** register the global hotkey and print a line on each press.
- **Touches:** `crates/nav-input/src/{hotkey.rs, thread.rs}`,
  orchestrator wiring.
- **Done when:**
  - Pressing `Alt+;` from any focused app prints
    `[input] hotkey id=0 latency_ns=...`.
  - `MOD_NOREPEAT` confirmed: holding the chord prints once, not a stream.
  - Conflict path tested: kill another tool that owns `Alt+;`, our launch
    surfaces an error to console (and later: tray balloon).

### B3. UIA enumeration baseline (no caching yet)

- **Goal:** simplest possible UIA enumeration to prove COM plumbing works.
- **Touches:** `crates/nav-uia/src/{lib.rs, runtime.rs, enumerate.rs,
  pattern.rs, coords.rs}`.
- **Done when:**
  - On `Alt+;`, log the count of `Invoke`-pattern elements and the time
    elapsed. Test against Notepad and File Explorer.
  - This *will* be slow (50–300ms). Record the numbers; this is our
    baseline against which Phase D's optimization is measured.
- **Anti-goal:** do **not** attempt to optimize here. We need the slow
  baseline numbers for the regression report.

**Phase B exit criterion:** Press hotkey, see element count and timing in
the log. No overlay yet.

---

## Phase C — First overlay

### C1. `nav-render` boot: empty layered window

- **Goal:** a transparent, click-through, top-most window covers the primary
  monitor, paints nothing, hides on Esc.
- **Touches:** `crates/nav-render/src/{lib.rs, overlay.rs, monitors.rs,
  device.rs}`.
- **Done when:**
  - Overlay shows on hotkey, dismisses on Esc.
  - `WS_EX_TRANSPARENT` confirmed (clicks pass through to underlying app).
  - DPI: a 100×100 px rect drawn at the top-left looks correct on a 100% and
    a 175% monitor.

### C2. Direct2D + DirectComposition paint path

- **Goal:** draw filled rounded rectangles with text inside. The first
  visible "hint".
- **Touches:** `crates/nav-render/src/{d2d.rs, scene.rs}`.
- **Done when:**
  - On hotkey, draw 5 hardcoded hints across the screen with labels
    `aa, ab, ac, ad, ae`.
  - Frame time per `update()` < 4 ms on a stock laptop GPU
    (dxgi vsync excluded).
  - Resize: dragging the source window does not flicker our overlay.

### C3. Wire enumeration to render

- **Goal:** real hints, not hardcoded.
- **Touches:** orchestrator, planner, render-input glue.
- **Done when:**
  - Hotkey on Notepad shows hints over `File`, `Edit`, `View`, `Help`,
    plus the close button. Type `aa` (or whatever the planner assigned),
    `File` opens.
  - Esc cancels with no stuck overlay.
  - Re-press hotkey while overlay visible cancels and restarts cleanly.

**Phase C exit criterion:** Working end-to-end MVP. Slow, but correct.
Demo: open Notepad, hit `Alt+;`, type two letters, the menu opens. We are
shippable in *concept* at this point.

### Phase C — implementation notes (repo state)

- **C1–C3** are implemented in `crates/nav-render`, `nav-input`, `nav-uia`,
  `nav-app`, and `nav-core` (orchestration currently in `nav-app/src/main.rs`).
- **Esc** cancels the session in the app (hint-mode LL hook + `Session::cancel`);
  the render thread no longer polls Esc independently.
- **Overlay DXGI:** layered popups use **`IDXGIFactory2::CreateSwapChainForComposition`**
  (not `CreateSwapChainForHwnd`) plus DComp `CreateTargetForHwnd`, after
  **`SetLayeredWindowAttributes(..., LWA_ALPHA)`**. **`WS_EX_NOREDIRECTIONBITMAP`**
  is **omitted** here because it caused `DXGI_ERROR_INVALID_CALL` on common
  stacks with flip-model swap chains; see **ADR-0015** in `14-risks-and-decisions.md`.
- **Hint labels** are assigned by `nav-core::plan` (alphabet
  `sadfjklewcmpgh` in the app today); the doc “type `aa`” example is illustrative
  only — use the two-letter label shown on each pill.

---

## Phase D — Make it fast

### D1. UIA cache request

- **Goal:** the single biggest perf win. Build the cache request once at
  startup; rely on it for every enumeration.
- **Touches:** `nav-uia/runtime.rs`, `nav-uia/enumerate.rs`, `nav-uia/cache.rs`.
- **Implemented:** `IUIAutomation::CreateCacheRequest` at `UiaRuntime::new`,
  `FindAllBuildCache` + `GetCachedPattern` for Invoke; `TreeScope_Element` on the
  request per Microsoft’s `FindAllBuildCache` contract.
- **Done when:**
  - Reference window enumeration drops from B3's baseline to **≤ 25 ms P95**.
  - Bench `enumerate_real` shows the delta in the regression report.

### D2. Pre-warm everything at boot

- **Goal:** first hotkey is as fast as the thousandth.
- **Touches:** `nav-app/main.rs`, `nav-uia/runtime.rs`,
  `nav-render/overlay.rs`.
- **Implemented:** `Renderer::prewarm()` → `RenderCmd::Prewarm`; overlay thread
  creates/positions the layered HWND, builds `D2dCompositionRenderer` once,
  runs `update_and_present(&[])` while hidden, then `ShowWindow(SW_HIDE)`;
  `hide_overlay` no longer drops the GPU; `Shutdown` clears `gpu` before
  `DestroyWindow`.
- **Done when:**
  - At app launch we eagerly: init COM in workers, build UIA cache request,
    create overlay windows hidden, init D3D/D2D devices, allocate brushes
    and the default text format.
  - Cold P95 ≤ warm P95 + 5 ms.

### D3. Parallelize enumeration where it pays off

- **Goal:** cut wall time on giant trees.
- **Touches:** `nav-uia/enumerate.rs` with rayon.
- **Implemented:** `FindAllBuildCache(Descendants).Length() ≥ 256`, root has `> 1`
  child, and **≥ 2** distinct `CurrentNativeWindowHandle` values (excluding the
  root HWND) → Rayon enumerates each HWND subtree on a **per-thread STA** worker
  with its own `IUIAutomation` + cache; non-HWND children merge on the main
  thread via `FindAllBuildCache` from the child element; `RawHint` carries
  `uia_invoke_hwnd` / `uia_child_index`; invoke uses `FindAllBuildCache` +
  `GetCachedPattern(Invoke)` with the same cache request.
- **Done when:**
  - For trees < 256 elements: keep the synchronous path (parallelism cost
    > benefit).
  - For trees ≥ 256: split children of the root across a rayon pool.
  - No regression on small trees; ≥ 30% wall-time reduction on the
    Visual Studio reference window.

### D4. Render diffs, not redraws

- **Goal:** filtering keystrokes update only changed quads.
- **Touches:** `nav-render/scene.rs`.
- **Implemented:** `paint_plan` compares last vs new pill geometry by **label** (unique per
  session); `NoOp` skips `BeginDraw`/`Present`; `Partial` uses `PushAxisAlignedClip` + transparent
  `FillRectangle` over the dirty union + redraws only pills intersecting the clip;
  large diffs or empty/old transitions use **full** clear + draw (`d2d.rs`).
- **Done when:**
  - During filter mode, `update()` measured at < 1.5 ms P95.
  - Frame trace shows only damaged regions are re-encoded.

**Phase D exit criterion:** P95 hotkey-to-hint < 30 ms on the reference set.
The standard target. We are now in shipping range for v1.

---

## Phase E — Compatibility and polish

### E1. MSAA fallback (`fallback_msaa.rs`)

- **Goal:** when UIA returns 0 elements (some legacy Win32 dialogs, DirectUI
  shell stuff), fall back to `IAccessible::accChild` walking.
- **Done when:**
  - The Win32 "Run" dialog (`Win+R`) shows hints on the OK / Cancel /
    Browse buttons.
  - Old MFC dialogs from the test fixture set show coverage ≥ 90%.

### E2. Raw-HWND fallback (`fallback_hwnd.rs`)

- **Goal:** last resort. `EnumChildWindows`, filter visible + enabled,
  treat as `GenericClickable` and click via `SendInput` at center.
- **Done when:**
  - The MSAA-failing fixtures still get hints.
  - This path is **never** chosen unless the prior two yielded zero.

### E3. Multi-monitor + per-monitor DPI

- **Goal:** correct on a 4K + 1080p mixed setup.
- **Done when:**
  - Drag the foreground window across monitors, hotkey works, hints show
    on the correct monitor at the correct DPI.

### E4. Tray icon + reload + quit

- **Goal:** a discoverable surface that does not require the user to know
  the binary is running.
- **Done when:**
  - Tray icon present, right-click menu has `Reload config`, `Open config`,
    `About`, `Quit`.
  - Editing the config file and clicking `Reload` re-registers the hotkey
    without restart.

### E5. Single-binary release artifact

- **Goal:** the shippable thing.
- **Done when:**
  - GitHub Actions release workflow produces a signed `.exe`, a `.zip`
    (portable), and an `.msix` (modern installer).
  - Cold-start to ready ≤ 150 ms on the reference machine.

**Phase E exit criterion:** v1.0 release candidate. All targets in
`00-overview.md` met. Document hand-off.

---

## Phase F — Elite (post-v1)

### F1. Glyph atlas + instanced text rendering

- **Goal:** push hint render into the elite (≤ 16 ms P95) range.
- **Touches:** `nav-render/glyph_atlas.rs`.
- **Done when:**
  - Pre-rasterize the alphabet at the configured font size into a single
    DXGI texture. Render hints as instanced quads sampling the atlas.
  - P95 render submit < 0.8 ms.

### F2. Progressive reveal

- **Goal:** for the rare giant tree, first hints visible at 8 ms even when
  total enumeration runs to 30 ms.
- **Done when:**
  - Worker emits hint chunks every ~4 ms. Render thread shows visible
    hints, patches in stragglers.
  - User-visible: starts as a few hints, fills in over 1–2 frames.

### F3. ETW provider

- **Goal:** profileable in the wild without rebuilding.
- **Done when:**
  - Custom ETW provider with events for `enum_start`, `enum_end`,
    `render_present`, `invoke_start`, `invoke_end`. Each event carries
    counts/sizes.
  - `tools/flame.ps1` captures a session and produces a flame graph.

### F4. UIA cache invalidation under change

- **Goal:** if the foreground app's tree mutates between sessions
  (e.g. dropdown opened, new dialog), our perf-optimal cache must be
  invalidated cleanly.
- **Done when:**
  - Subscribing to `UIA_StructureChangedEventId` invalidates the cache for
    that HWND. Next session re-builds.

**Phase F exit criterion:** P95 ≤ 16 ms, P99 ≤ 24 ms on the reference set.
This is the elite version.

---

## Daily checklist (post-Phase B)

Run **before each commit**:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Run **before each PR**:

```powershell
cargo bench -p nav-bench --bench label --bench filter -- --baseline main
.\tools\bench-runner.ps1 -Compare main
```

If any benchmark regresses by more than 5% P95, the PR must explain why in
its description. No silent regressions.
