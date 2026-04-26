# 14 — Risks and Decisions

> Architecture Decision Records (ADRs) for choices we made on purpose, plus
> a registry of known risks with mitigations. New decisions get appended;
> older ones never get edited (only superseded). This is the "why we did
> what we did" log.

## ADR format

```
### ADR-NNNN: <Title>
- Status:   accepted | superseded by ADR-XXXX | deprecated
- Date:     YYYY-MM-DD
- Context:  why we had to decide
- Decision: what we did
- Consequences: what we accept by deciding this
```

---

## ADRs

### ADR-0001: Rewrite in Rust, not refactor C#
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Legacy HAP is C# / WPF / .NET Framework 4.x. Hot-path is
  dominated by per-element COM round-trips and WPF first-paint cost.
  Cold start is 1.5-3 s; hotkey-to-pixels is 200-300 ms.
- **Decision:** Full rewrite in Rust. Native Win32 / COM directly. Direct2D +
  DirectComposition for rendering. No .NET runtime in the hot path.
- **Consequences:**
  - We pay a one-time cost to rebuild instead of porting incrementally.
  - We lose any future user wanting a "pure C#" version.
  - We gain a sub-30 ms latency target that WPF cannot meet.
  - We gain a single static binary (~3 MB) with no runtime dependency.

### ADR-0002: Single process, no background service
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Some hint tools split into a daemon + CLI. This adds
  install complexity, IPC latency, and UAC interaction matrix.
- **Decision:** Navigator is a single user-mode process. Hotkey is global
  via `RegisterHotKey`; that does not need a service.
- **Consequences:**
  - Cannot hint elevated windows from a non-elevated client (UIPI).
    We document and refer users to a "Run elevated" tray option.
  - The user must re-launch Navigator on each login (until we add
    autostart in M15).

### ADR-0003: UIA cache request is mandatory
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** `IUIAutomationCacheRequest` cuts enumeration latency by
  ~80%. The legacy code does not use it.
- **Decision:** Build the cache request once at startup. Use
  `BuildUpdatedCache` for every enumeration. Walk the cached subtree
  locally — no COM calls during the walk.
- **Consequences:** Every property/pattern we ever query must be added to
  the cache request once at boot. If we add a new pattern in v2, we update
  the cache request build site.

### ADR-0004: DirectComposition + Direct2D for rendering
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** We need a flicker-free, click-through, vsync-aligned overlay
  with sub-millisecond redraws. WPF, GDI, and Skia all have disqualifying
  trade-offs.
- **Decision:** One layered window per monitor with
  `WS_EX_NOREDIRECTIONBITMAP`. DComp tree backed by D3D11 swap chains.
  Direct2D context per monitor.
- **Consequences:**
  - Win10 floor is 1809 (DComp animations dependency).
  - We accept the larger Windows SDK feature surface.

### ADR-0005: Low-level keyboard hook only during hint mode
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** LL hooks tax every keystroke globally. Some tools install
  them permanently to capture chord sequences; this measurably harms
  global typing latency on slow machines.
- **Decision:** Install `WH_KEYBOARD_LL` *only* between session start and
  session end. Outside hint mode we rely on `RegisterHotKey`.
- **Consequences:**
  - Slightly more code to install/uninstall the hook on every session
    transition.
  - Idle keyboard latency is unchanged for Navigator users.

### ADR-0006: Snapshot foreground HWND before showing overlay
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Legacy HAP captured the HWND after creating the overlay
  window. On slow machines this raced — the overlay became foreground and
  HAP would enumerate itself.
- **Decision:** `GetForegroundWindow` is called inside the
  `WM_HOTKEY` handler, *before* any other work.
- **Consequences:**
  - The `Session` carries the HWND through every step.
  - The orchestrator polls `GetForegroundWindow` every 50 ms during a
    session and cancels if it changes.

### ADR-0007: No async/await
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** COM is blocking. An async runtime over blocking COM is
  pure overhead.
- **Decision:** Plain threads. `crossbeam_channel` for messaging.
  `rayon` for the worker pool.
- **Consequences:**
  - We lose `tokio`-style ecosystem polish in some places (e.g. file
    watchers). We use small, focused crates instead.

### ADR-0008: Per-monitor V2 DPI awareness via manifest
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Setting DPI awareness at runtime is a footgun on Win10
  because some HWNDs may already exist in the prior mode.
- **Decision:** Manifest-only. `app.manifest` declares
  `PerMonitorV2`.
- **Consequences:** No runtime DPI awareness override; we cannot retrofit
  if a downstream packager strips the manifest.

### ADR-0009: One alphabet, in priority order
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Some Vimium-style tools have separate "short" and "long"
  alphabets. Adds complexity for marginal benefit.
- **Decision:** A single ordered alphabet. Short labels are drawn from the
  start; long labels combine alphabet chars in the same order. Matches
  legacy HAP's behavior.
- **Consequences:** Cannot trivially configure "fingers-only" vs
  "fall-through" letters. If users complain, we revisit in v2.

### ADR-0010: TOML config, no GUI
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** A settings panel is a maintenance black hole.
- **Decision:** Single `config.toml`. Tray menu has "Open config",
  "Reload config", "Reset config".
- **Consequences:** Users must be willing to edit a text file. This
  matches the target audience.

### ADR-0011: `windows` crate, not `winapi`
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Two viable Win32 binding crates. Microsoft maintains
  `windows` and ships UIA / D2D / DComp first-class. `winapi` is in
  maintenance mode.
- **Decision:** `windows`.
- **Consequences:** Slightly more boilerplate for COM. Pinned version,
  bumped on a calendar.

### ADR-0012: `panic = "abort"` in release
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Unwinding through COM callbacks is undefined behavior
  territory. Stack unwinding adds binary size and is rarely useful for a
  desktop tool.
- **Decision:** `panic = "abort"`. A panic logs (best effort) and exits.
  The user re-triggers.
- **Consequences:** Lose the ability to recover from panics in tests
  using `catch_unwind`. We ban panics on the hot path; tests assert on
  `Result<_, _>` returns.

### ADR-0013: Reuse legacy hint algorithm
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Long-time HAP users have muscle memory built around the
  vimium-style label distribution and the 14-char alphabet.
- **Decision:** Port `HintLabelService::GetHintStrings` faithfully to
  `nav-core::label::generate_labels`.
- **Consequences:** First-time Navigator users get the same hint patterns
  that long-time HAP users expect. Future v2 may experiment, but only
  behind a config flag with the legacy algorithm as the default.

### ADR-0014: No telemetry. Period.
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** Power-user tools historically lose trust the moment they
  add "anonymous usage stats."
- **Decision:** Navigator never sends data anywhere. No analytics.
  No update pings (until M11, opt-in).
- **Consequences:** We cannot field-collect crash dumps. We rely on
  user-reported issues and `tracing` logs.

### ADR-0015: Composition swap chain + no `WS_EX_NOREDIRECTIONBITMAP` on MVP overlay
- **Status:** accepted
- **Date:** 2026-04-26
- **Context:** On common driver stacks, **`IDXGIFactory2::CreateSwapChainForHwnd`**
  on a **layered full-screen `WS_POPUP`** returned **`DXGI_ERROR_INVALID_CALL`**
  (`0x887A0001`) even with valid pixel dimensions and without
  `DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`. **`WS_EX_NOREDIRECTIONBITMAP`** made the
  failure mode worse in local testing. **`SetLayeredWindowAttributes(..., LWA_ALPHA)`**
  is still required so the HWND has a defined layered alpha path before DComp
  targets it.
- **Decision:** Create the flip-model swap chain with
  **`CreateSwapChainForComposition`**, attach it to the DComp visual with
  **`SetContent`**, and bind the visual tree to the overlay HWND with
  **`CreateTargetForHwnd`**. Omit **`WS_EX_NOREDIRECTIONBITMAP`** on this window
  class until we have a PIX-validated matrix that proves a safe combination.
  Do not pass **`DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`** unless
  **`CheckFeatureSupport(DXGI_FEATURE_PRESENT_ALLOW_TEARING)`** succeeds; use
  **`Present(1, 0)`** when tearing is off.
- **Consequences:**
  - Slightly more documentation drift vs the original "redirection bitmap"
    story in **ADR-0004** until we either restore the flag behind a probe or
    document "never" for this HWND class.
  - One extra layered-window setup call (`SetLayeredWindowAttributes`) on
    every show path that creates the GPU stack.

---

## Risks register

| ID  | Risk                                                                  | Likelihood | Impact | Mitigation                                                                         |
|-----|-----------------------------------------------------------------------|------------|--------|------------------------------------------------------------------------------------|
| R-1 | UIA enumeration is too slow on giant Electron windows.                | High       | High   | Cache request (M6); progressive reveal (M13); per-step time budgets.               |
| R-2 | DirectComposition pipeline misbehaves on driver edge cases.           | Medium     | Medium | Fallback to WARP renderer on `D3D11CreateDevice` failure; PIX-validated path.      |
| R-3 | LL keyboard hook is flagged by AV/EDR products.                       | Medium     | High   | Install only during hint mode; signed binary; documented compat list in README.    |
| R-4 | Hotkey conflict (another tool already owns `Alt+;`).                  | Medium     | Low    | Surface clear error to tray; recommend custom chord; never silently change it.     |
| R-5 | UIA returns 0 elements for some legacy Win32 dialogs.                 | Medium     | Medium | MSAA fallback (M9); raw HWND fallback (M9); diagnose dump for unsupported windows. |
| R-6 | Per-monitor DPI complications on multi-monitor setups with mixed DPI. | High       | Medium | M8 dedicated work, manual matrix testing on the reference machine.                 |
| R-7 | Foreground window changes mid-session (alert dialog steals focus).    | Medium     | Low    | 50 ms heartbeat; cancel cleanly; no stuck overlay.                                 |
| R-8 | Cold start exceeds budget on slow disks.                              | Low        | Medium | Pre-warm at first idle, not at boot; consider lazy init only behind a feature flag.|
| R-9 | UIA tree mutates between sessions (cached request still works, since we rebuild the cache subtree per session). | Low | Low | Per-session `BuildUpdatedCache`; no cross-session element cache.       |
| R-10 | UIPI prevents hinting elevated windows from non-elevated client.     | Certain    | Low    | Documented; tray "Run elevated" option in M15.                                     |
| R-11 | DWM compositor adds 1–2 vsync latency after our present.             | Certain    | Medium | Use `Present(0,0)` + DComp commit; document budget includes DWM in `00-overview`.  |
| R-12 | Anti-cheat in fullscreen games blocks all input hooks.                | Medium     | Low    | Document. Navigator is not designed for use during games.                          |
| R-13 | A future Windows update changes UIA behavior (rare but happens).      | Low        | High   | Pinned reference machine; weekly reliability matrix; release notes call out tested OS builds. |
| R-14 | Maintainer bandwidth: Navigator becomes a one-person project.         | Medium     | High   | Simple, small codebase; thorough docs (this folder); no exotic tooling.            |
| R-15 | Scope creep ("can we add macros?").                                   | High       | High   | Anti-goals enumerated in `00-overview.md` and `10-milestones.md`; reject in PR review. |

## Decision-making rules

For new architectural choices not yet captured here:

1. Open an ADR draft as a PR, format above.
2. The PR description must describe the alternatives considered and the
   trade-offs accepted.
3. At least one maintainer signs off.
4. Once merged, the ADR is **immutable**. Disagreements get a *new* ADR
   that supersedes the old one.

For new risks:

1. Append a row to the table.
2. Include a mitigation. A risk without a mitigation is a TODO that needs
   to be written first.
3. Re-rank quarterly during a maintainer review.

## Things we will reconsider after v1

(Not commitments — explicit "open questions" we have parked.)

- Whether to support Win32 mouse-mode (a "click hints" mode with no
  keyboard).
- Whether to support a small interpreter for chained actions
  ("type these letters, then those").
- Whether to expose UIA tree dumps for diagnosing unsupported windows.
- Whether a tiny optional service makes sense for autostart + elevated
  hints (a 100 KB native service, not a .NET host).

These are **not** v1 scope. Each gets a separate post-v1 design doc.
