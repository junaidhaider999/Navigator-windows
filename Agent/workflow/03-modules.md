# 03 — Modules

This document defines **what each crate is responsible for, what its public
API looks like, and what it must never do.** Treat these contracts as
load-bearing. Cross-crate calls happen through these interfaces only.

> Type signatures here are illustrative Rust. They will evolve, but the shape
> and the responsibility split must not.

---

## `nav-core` — pure logic

**Owns:** the domain model, the state machine, the hint label algorithm,
prefix filtering, scoring, error types.

**Forbidden imports:** `windows`, anything OS-specific. Pure, testable,
cross-platform.

### Public types

```rust
// hint.rs
#[derive(Clone, Debug, PartialEq)]
pub struct RawHint {
    pub element_id: u64,        // Stable id provided by enumerator (UIA runtime id hash, etc.)
    pub bounds: Rect,           // Screen-space, physical pixels.
    pub kind: ElementKind,      // What action this element supports.
    pub name: Option<Box<str>>, // Optional accessible name. Used only for ranking.
    pub backend: Backend,       // UIA / MSAA / RawHwnd
}

#[derive(Clone, Debug)]
pub struct Hint {
    pub raw: RawHint,
    pub label: Box<str>,        // Assigned by planner, e.g. "JK"
    pub score: f32,             // 0.0 = best, used to assign short labels first.
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ElementKind {
    Invoke,                     // Button-like.
    Toggle,                     // Checkbox-like.
    Select,                     // SelectionItem-like.
    ExpandCollapse,             // Tree node, dropdown.
    Editable,                   // Edit control; we focus rather than click.
    GenericClickable,           // Last resort: click at center.
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Backend { Uia, Msaa, RawHwnd }

#[derive(Copy, Clone, Debug)]
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
```

### Public functions

```rust
// label.rs
pub fn generate_labels(
    count: usize,
    alphabet: &[char],          // E.g. ['s','a','d','f','j','k','l','e','w','c','m','p','g','h']
) -> Vec<Box<str>>;

// planner.rs
pub fn plan(
    raws: Vec<RawHint>,
    alphabet: &[char],
    layout_origin: Rect,        // Used for proximity scoring.
) -> Vec<Hint>;

// filter.rs
pub fn filter<'a>(hints: &'a [Hint], prefix: &str) -> FilterResult<'a>;

pub enum FilterResult<'a> {
    None,                       // No match. Cancel.
    Many(Vec<&'a Hint>),        // Still ambiguous. Render mask.
    Single(&'a Hint),           // Unique match. Invoke.
}

// session.rs
pub struct Session { /* ... */ }

impl Session {
    pub fn new(seed: u64) -> Self;
    pub fn ingest(&mut self, hints: Vec<Hint>);
    pub fn key(&mut self, c: char) -> SessionEvent;
    pub fn cancel(&mut self) -> SessionEvent;
}

pub enum SessionEvent {
    Render(Vec<HintId>),        // Redraw with this visible mask.
    Invoke(HintId),
    Done,
}
```

### Invariants

- All `Rect`s in `nav-core` are in **physical pixels**. DPI conversion happens
  in the boundary crates (`nav-uia`, `nav-render`).
- `Session::key` is **deterministic and pure** in the sense that given the
  same prior session and the same key, the result is always the same.
- The label generator never returns labels containing characters outside the
  provided alphabet.
- Labels are **prefix-free** (no label is a prefix of another, except by
  identical label). This is what makes typing work without timeouts.

### Anti-responsibilities

- ❌ No COM, no Win32, no rendering, no I/O.
- ❌ No global state. The orchestrator passes context in, receives results out.
- ❌ No allocations in `Session::key`'s hot path beyond the result Vec
  (see `12-benchmarking.md` for the budget).

---

## `nav-config` — configuration

**Owns:** `config.toml` schema, defaults, discovery order, CLI parser.

### Public API

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub hotkey: HotkeySpec,
    pub alphabet: String,                // "sadfjkle wcmpgh"
    pub font_size_px: u32,               // Default 14.
    pub colors: Colors,
    pub exclusions: Exclusions,
    pub fallback: FallbackConfig,
    pub log: LogConfig,
}

pub fn load(args: &CliArgs) -> Result<Config, ConfigError>;
pub fn write_default_to(path: &Path) -> Result<()>;
```

### Discovery order (highest precedence wins)

1. CLI flag (`--hotkey ctrl+;`)
2. `--config <path>` flag
3. `%APPDATA%\Navigator\config.toml`
4. `<exe-dir>\config.toml` (portable mode)
5. Embedded defaults (`include_str!("../../assets/default-config.toml")`)

### Anti-responsibilities

- ❌ Does not touch disk on the hot path. Config is loaded once at startup,
  cached in an `Arc<Config>` and passed to modules.
- ❌ Does not validate Win32 hotkey availability — that's `nav-input`'s job.

---

## `nav-uia` — UI Automation

**Owns:** all COM/UIA interaction. Element enumeration. Pattern execution.
Coordinate transforms. The MSAA and raw-HWND fallbacks live here too because
they share rect/coordinate concerns.

### Public API

```rust
pub struct UiaRuntime { /* IUIAutomation, cache request, cached props */ }

impl UiaRuntime {
    pub fn new() -> Result<Self, UiaError>;          // Heavy. Call once at startup.

    pub fn enumerate(
        &self,
        hwnd: HWND,
        opts: &EnumOptions,
    ) -> Result<Vec<RawHint>, UiaError>;

    pub fn invoke(&self, hint: &Hint) -> Result<(), UiaError>;
}

pub struct EnumOptions {
    pub max_elements: usize,        // Hard cap, default 1024.
    pub include_offscreen: bool,    // Default false.
    pub include_disabled: bool,     // Default false.
    pub fallback: FallbackPolicy,   // Auto / UiaOnly / MsaaOnly.
}
```

### Internal pipeline (must remain in this order)

1. `IUIAutomation::ElementFromHandle(hwnd)` — root.
2. Build / fetch the cached `IUIAutomationCacheRequest` (created **once** at
   startup, not per call). Caches: `BoundingRectangle`, `IsEnabled`,
   `IsOffscreen`, `ControlType`, `ClassName`, plus `InvokePattern`,
   `TogglePattern`, `SelectionItemPattern`, `ExpandCollapsePattern`,
   `ValuePattern`.
3. `BuildUpdatedCache` to inflate the root with cached descendants — this is
   the **single biggest perf win** vs the legacy `FindAll` + per-element COM
   calls.
4. Walk the cached tree synchronously. **No COM calls** during the walk —
   everything reads from the cache.
5. Convert UIA bounding rects (screen-space, physical pixels) to `RawHint`
   structs.
6. If element count is 0, fire the MSAA fallback. If still 0, fire the raw
   HWND walker.

### Coordinate handling

- UIA's `BoundingRectangle` is screen-space, physical pixels, and respects
  per-monitor DPI awareness because we declared `PerMonitorV2` in the
  manifest. **Do not call `LogicalToPhysicalPointForPerMonitorDPI` or its
  cousins.** They are footguns and are wrong on multi-monitor setups.
- The render layer is also per-monitor DPI aware, so we pass physical
  pixels straight through.

### Anti-responsibilities

- ❌ Does not own threads. `nav-app` decides which thread/pool calls into it.
- ❌ Does not render. Returns data only.
- ❌ Does not retry on COM errors (orchestrator's job).

---

## `nav-input` — hotkeys and key capture

**Owns:** registration of the global hotkey, the message-only window that
receives `WM_HOTKEY`, the low-level keyboard hook installed during hint mode,
keyboard layout-aware character translation.

### Public API

```rust
pub struct InputThread { /* opaque */ }

pub enum InputEvent {
    Hotkey,                          // Primary trigger.
    DebugHotkey,                     // M2 dev-only (debug enumeration).
    HintKey(char),                   // While in hint mode.
    HintCancel,                      // Esc / focus lost / second hotkey.
}

impl InputThread {
    pub fn spawn(cfg: &Config) -> Result<(Self, Receiver<InputEvent>)>;
    pub fn enter_hint_mode(&self);   // Installs LL hook, swallows non-hint keys.
    pub fn exit_hint_mode(&self);    // Uninstalls hook.
    pub fn shutdown(self);
}
```

### Hotkey registration

- Primary: `RegisterHotKey(hwnd, id, modifiers, vk)`. Lowest latency.
- Modifier defaults: `MOD_ALT | MOD_NOREPEAT`, VK = `VK_OEM_1` (semicolon).
  Matches HAP's `Alt+;` to keep muscle memory.
- `MOD_NOREPEAT` is mandatory; without it, holding the hotkey storms us.
- If `RegisterHotKey` fails (another app owns the chord), surface a clear
  error to the tray. Do **not** silently change the hotkey.

### Low-level hook lifecycle

- The LL hook is **only installed during hint mode.** Outside of hint mode
  Navigator does not touch the global keyboard stream. This matters: LL
  hooks are scrutinized by AV products and have a timeout the OS enforces
  (`LowLevelHooksTimeout` registry value, default 300ms). If our hook
  callback takes too long, Windows silently un-installs us.
- Inside hint mode the hook:
  - Eats all keydown messages (returns `1` from `CallNextHookEx`-bypass)
    so the underlying app does not see them.
  - Translates VK → char respecting current layout via `ToUnicodeEx`.
  - Forwards `Esc`, the hotkey itself (re-press cancels), and unknown keys
    upstream, with a configured policy.
- The hook callback **must do no work** beyond layout translation and a
  `crossbeam_channel::try_send`. Anything else is paid for by the user as
  global keyboard latency.

### Anti-responsibilities

- ❌ No element enumeration, no rendering, no policy decisions about what to
  do with a key — that is the orchestrator + `nav-core::Session`.

---

## `nav-render` — overlay rendering

**Owns:** all per-monitor layered windows, the D3D11 device, the Direct2D
device context, the DirectComposition tree, glyph atlas.

### Public API

```rust
pub struct Renderer { /* owns the render thread and the windows */ }

impl Renderer {
    pub fn spawn(cfg: &Config) -> Result<Self, RenderError>;

    pub fn show(&self, session_id: u64, hints: &[Hint]) -> Result<(), RenderError>;
    pub fn repaint(&self, session_id: u64, hints: &[Hint]) -> Result<(), RenderError>;
    pub fn hide(&self, session_id: u64) -> Result<(), RenderError>;

    pub fn shutdown(self);
}
```

### Window properties (each layered overlay)

```
WS_POPUP
WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST
| WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
```

- `WS_EX_NOACTIVATE` — never steals focus, never appears in Alt+Tab.
- `WS_EX_TRANSPARENT` — input passes through us. Keys are captured by the
  LL hook in `nav-input`, not by this window.
- `WS_EX_NOREDIRECTIONBITMAP` — **not** set in the current overlay: DXGI flip
  swap chains failed to create on layered popups when combined with it; see
  **ADR-0015** in `14-risks-and-decisions.md`.
- `WS_EX_TOOLWINDOW` — keeps us out of the taskbar.

### Lifecycle

1. **Boot** (during app startup, before first hotkey): **target** design —
   create one overlay per monitor, hidden, and pre-warm D3D/D2D/DComp (**M7**).
   **Current code:** the overlay HWND is created on first `Show`; the
   `D2dCompositionRenderer` is constructed there (first hotkey pays GPU init).
2. **Show**: position overlay over the primary monitor, build a scene from
   `Vec<Hint>`, encode draw list, present, `DComp::Commit`.
3. **Update**: re-encode visible quads only, swap buffer, commit. The full
   tree is not rebuilt.
4. **Hide**: collapse the DComp visual to invisible. Do **not** destroy the
   window; reuse it next session.
5. **Shutdown**: tear down on app exit only.

### Why DirectComposition

- Bypasses GDI redirection. No flicker, no compositor surprises.
- DWM composes our visual with vsync alignment. Predictable presentation
  latency.
- Lets us use `IDCompositionTarget::SetRoot` with a `IDCompositionVisual`
  backed by a swap chain — the GPU does the work, the CPU stays free.

### Anti-responsibilities

- ❌ Does not know what a "hotkey" is.
- ❌ Does not know what an action is. It receives `Hint`s and shows them.
- ❌ Does not invoke clicks. Does not allocate hint labels.

---

## `nav-app` — orchestration and the binary

**Owns:** `main`, single-instance lock, tray icon, logging init, the
orchestrator that owns the session lifetime and routes events between the
other crates.

### Public API

This crate exposes nothing — it is the executable. `main.rs` is short:

```rust
fn main() -> ExitCode {
    logging::init();
    let _guard = single_instance::acquire()?;
    let cfg = nav_config::load(&CliArgs::parse())?;

    let (input, rx_input) = InputThread::spawn(&cfg)?;
    let renderer          = Renderer::spawn(&cfg)?;
    let uia               = UiaRuntime::new()?;
    let mut orch          = Orchestrator::new(cfg, input, renderer, uia);

    orch.run(rx_input);
    ExitCode::SUCCESS
}
```

### Orchestrator responsibilities

- Holds the foreground HWND snapshot.
- Drives the session state machine (`nav-core::Session`).
- Handles fallback escalation on enumeration failure.
- Owns the tray icon and its context menu (Reload Config, Quit, Open
  Config Folder).

### Anti-responsibilities

- ❌ Does not implement enumeration, rendering, or input itself. It is glue.
- ❌ Does not parse config (delegates to `nav-config`).

---

## `nav-bench` — performance harness

**Owns:** Criterion benches, synthetic UIA mocks, real-target drivers.

Benches are organized so that we can answer:

- **Pure-Rust benches** (run on every CI build): label generation, filter,
  planner. Sub-microsecond targets.
- **Synthetic enumeration bench**: 10k-element mock tree. Measures the
  *non-COM* portions of the enumerator.
- **Real-target bench** (manual / scheduled): launches a fixture WinForms
  app with a known element count, drives the full pipeline 100 times,
  reports P50/P95/P99.

See `12-benchmarking.md` for the methodology and the regression gate.
