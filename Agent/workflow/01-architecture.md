# 01 — Architecture

## Bird's-eye view

Navigator is a **single-process, multi-threaded native binary** organized as a
small set of orthogonal modules. There is no service, no IPC, no daemon. The
entire engine lives in one OS process and communicates through lock-free
channels.

```
                ┌────────────────────────────────────────────────────────┐
                │                     nav-app (binary)                   │
                │                                                        │
   keypress ──▶ │  ┌──────────────┐    Trigger    ┌─────────────────┐    │
                │  │  Input thread│──────────────▶│ Orchestrator    │    │
                │  │ (WM_HOTKEY,  │               │ (state machine) │    │
                │  │  LL hook)    │◀──────────────│                 │    │
                │  └──────────────┘  Done/Cancel  └────────┬────────┘    │
                │                                          │             │
                │                                          ▼             │
                │  ┌────────────────────────────────────────────────┐    │
                │  │              Worker pool (rayon)               │    │
                │  │  ┌──────────────────┐  ┌───────────────────┐   │    │
                │  │  │ UIA enumerator   │  │ MSAA / fallback   │   │    │
                │  │  │ (cached batches) │  │ enumerator        │   │    │
                │  │  └─────────┬────────┘  └─────────┬─────────┘   │    │
                │  │            └──────┬──────────────┘             │    │
                │  └───────────────────┼────────────────────────────┘    │
                │                      ▼                                 │
                │  ┌──────────────┐  Hints   ┌──────────────────────┐    │
                │  │ Hint planner │─────────▶│   Render thread       │    │
                │  │ (labels +    │          │   (D2D + DComp)      │    │
                │  │  ranking)    │          └──────────┬───────────┘    │
                │  └──────────────┘                     │ DWM commit     │
                │                                       ▼                │
                │                                  Composited overlay    │
                └────────────────────────────────────────────────────────┘
                                          │
                                          ▼
                                     Target HWND
                              (UIA invoke / SendInput click)
```

## Threading model

We use **four threads, no more, no fewer** in the hot path. Every additional
thread is a synchronization tax we have to pay later.

| Thread        | Owner                  | Priority             | Responsibility                                     |
|---------------|------------------------|----------------------|----------------------------------------------------|
| `main`        | OS / app entry         | Normal               | Init, shutdown, tray icon message pump.            |
| `input`       | nav-input              | `THREAD_PRIORITY_ABOVE_NORMAL` | Owns the hidden message-only window for `WM_HOTKEY`, runs the LL keyboard hook during hint mode. |
| `render`      | nav-render             | `THREAD_PRIORITY_ABOVE_NORMAL` | Owns the layered overlay window, the D3D11 device, the D2D context, DirectComposition tree. |
| `worker[0..N]`| nav-uia (rayon pool)   | Normal               | Parallel UI Automation enumeration and pattern queries. `N = max(2, num_cpus - 2)`. |

**Rules:**

- `input` thread **never blocks** on enumeration or rendering. It posts work
  and returns immediately.
- `render` thread **never blocks** on UIA. UIA results arrive via a SPSC
  channel (`crossbeam::channel::bounded(1)`), the render thread coalesces
  partial updates.
- `worker` threads **never touch UI**. They produce `Vec<RawHint>` and post.
- COM apartment: `input` and `render` initialize as `COINIT_APARTMENTTHREADED`
  (STA), workers as `COINIT_MULTITHREADED` (MTA). Cross-apartment marshalling
  is paid once at boundary, never in the hot loop.

## Lifecycle of a hint session

A "hint session" is the canonical unit of work. The orchestrator drives it
through this state machine:

```
        ┌──────────┐  hotkey   ┌─────────────┐   first hint    ┌──────────┐
        │   Idle   │──────────▶│ Enumerating │────────────────▶│ Visible  │
        └──────────┘           └──────┬──────┘                 └────┬─────┘
             ▲                        │ no hints                    │ key
             │                        ▼                             ▼
             │                  ┌──────────┐    Esc / blur   ┌──────────┐
             │                  │ NoHints  │◀────────────────│ Filtered │
             │                  └────┬─────┘                 └────┬─────┘
             │                       │                            │ match
             │                       │                            ▼
             │                       │                       ┌──────────┐
             │                       │                       │ Invoking │
             │                       │                       └────┬─────┘
             │                       ▼                            │ done
             └───────────────────────┴────────────────────────────┘
```

State transition rules:

- **Idle → Enumerating**: hotkey arrives. Capture foreground HWND *before*
  any other work; the moment we render, we steal focus. Snapshot is the truth.
- **Enumerating → Visible**: first batch of hints ready (we render
  progressively if enumeration runs long; see §"Progressive reveal").
- **Visible → Filtered**: each keypress narrows the candidate set.
- **Filtered → Invoking**: candidate set has exactly one element, OR a
  unique full-prefix match.
- **Invoking → Idle**: on success or failure, always tear down overlay,
  restore foreground, drop session resources.
- **Any → Idle (Cancel)**: Esc, focus loss, timeout (configurable, default
  off), or second hotkey press.

## Data flow

```
  hotkey ──▶ orchestrator.start(captured_hwnd)
                │
                ├─▶ uia.enumerate(hwnd) ──┐
                │                         │ Vec<RawHint>
                ├─▶ msaa.enumerate(hwnd) ─┤  (merged, deduped by rect)
                │                         │
                ├─▶ raw_hwnd.enumerate ───┘
                │
                ▼
        planner.build(raw_hints, alphabet, layout) ──▶ Vec<Hint>
                                                         │
                                                         ▼
                                         render.show(session_id, hints)
                                                         │
       keypress ──▶ orchestrator.on_key(c) ──▶ planner.filter(prefix)
                                                         │
                                                         ▼
                                         render.update(session_id, mask)
                                                         │
       match ──▶ orchestrator.invoke(hint) ──▶ uia/msaa/sendinput
                                                         │
                                                         ▼
                                         render.hide(session_id)
```

## Module boundaries

The crate split is the architecture. Cross-crate calls are the only allowed
boundaries; same-crate sibling modules are implementation detail.

| Crate         | Depends on                        | Knows about Win32?  |
|---------------|-----------------------------------|---------------------|
| `nav-core`    | std, serde                        | No                  |
| `nav-config`  | nav-core, toml, serde, clap       | No                  |
| `nav-uia`     | nav-core, windows, parking_lot    | Yes (UIA only)      |
| `nav-input`   | nav-core, windows                 | Yes (User32, hooks) |
| `nav-render`  | nav-core, windows                 | Yes (D2D/D3D/DComp) |
| `nav-app`     | all of the above                  | Glue only           |
| `nav-bench`   | nav-* (test-only)                 | Yes                 |

**`nav-core` is the keystone.** It is pure logic, fully testable on Linux/macOS,
contains the hint planner, label generator, ranking, state machine. No `windows`
crate import is allowed in `nav-core`. This is enforced by `cargo deny` in CI.

## Why this architecture

**Single process, not service.** A service costs ~5MB RSS at idle, requires
install rights, and complicates UAC scenarios. It buys nothing — UIA does not
need elevation for non-elevated targets, and we explicitly do not support
hinting elevated windows from a non-elevated client (UIPI prevents it; document
and move on).

**Dedicated render thread, not main-thread WPF.** WPF/UWP renderers run on
the dispatcher and are at the mercy of GC pauses. Direct2D + DirectComposition
on a dedicated thread gives us a per-frame budget we can actually hold.

**Rayon pool, not async/await.** UIA is COM-blocking; async runtimes do not
help blocking COM calls. A bounded thread pool with work-stealing is the
correct tool. We do not use `tokio` anywhere in this project.

**Snapshot the foreground HWND first.** The legacy HAP captured the HWND
*after* showing the overlay. That is a race: on a slow machine, the overlay
becomes the foreground, and we then try to enumerate ourselves. Capture
*before* any visual work.

**No global mutable state.** Each session is a value, owned by the
orchestrator, freed at session end. This is the discipline that prevents
"stuck overlay" bugs.

## Progressive reveal (advanced, M5+)

If enumeration exceeds 8 ms (rare but real on giant Electron windows), the
worker pool emits `Vec<RawHint>` chunks every ~4 ms. The render thread shows
visible hints immediately and patches in stragglers. This trades a ~3% extra
draw call cost for a much better worst-case feel: no app ever feels frozen.

This is **off by default in M2-M4** and gated behind a config flag in M5.
Implement only after the simple synchronous path is correct.

## Failure modes and recovery

| Failure                                       | Detection                              | Recovery                                            |
|-----------------------------------------------|----------------------------------------|-----------------------------------------------------|
| UIA returns empty for a window with elements  | Element count == 0 after enumeration   | Run MSAA enumerator, then raw HWND walker.          |
| UIA throws `RPC_E_CALL_REJECTED`              | COM HRESULT                            | Retry once, then fall back to MSAA.                 |
| Foreground window changes mid-enumeration     | `GetForegroundWindow() != captured`    | Cancel session silently, restore.                   |
| LL keyboard hook gets timeout-revoked by OS   | `SetWindowsHookExW` returns null on retry | Log, fall back to RegisterHotKey-only chord.     |
| Overlay window fails to create (rare)         | `CreateWindowExW` returns null         | Log, beep, return to Idle. Never crash.             |
| Invoke pattern throws                         | COM HRESULT                            | Try Toggle → Select → SendInput click at center.    |

The orchestrator owns the recovery logic. Each module returns `Result<T, E>`
with a precise error type. **Never `unwrap()` in the hot path.**
