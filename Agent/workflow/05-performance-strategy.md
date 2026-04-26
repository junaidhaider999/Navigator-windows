# 05 — Performance Strategy

> The product is latency. This document is where the milliseconds come from
> and how we keep them.

## Where the time goes (legacy HAP, instrumented)

Profiled HAP on a stock Win11 desktop pressing `Alt+;` over Visual Studio:

| Stage                                            | Median   | Notes                                              |
|--------------------------------------------------|----------|----------------------------------------------------|
| WM_HOTKEY → handler                              |   2 ms   | .NET dispatcher dispatch, GC.                      |
| `IUIAutomation.ElementFromHandle`                |   3 ms   | One COM round-trip.                                |
| `FindAll(TreeScope_Descendants, ...)`            | 110 ms   | **The wall.** Per-element COM round-trips.         |
| Per-element pattern queries (`GetCurrentPattern`)|  85 ms   | 5 patterns × N elements × COM cost.                |
| `BoundingRectangle`/`IsEnabled`/`IsOffscreen`    |  40 ms   | One COM call per property per element.             |
| WPF overlay `Show()`                             |  18 ms   | Window create, dispatcher work, layout.            |
| WPF first paint                                  |  35 ms   | Software-rasterized text and brushes.              |
| **Total**                                        | **293 ms** | Felt. Hateful.                                   |

Two findings drive Navigator's architecture:

1. **80% of the time is COM round-trips that did not have to happen.** Each
   property access on a `IUIAutomationElement` is an out-of-process call to
   the target app. Multiply by N elements × M properties.
2. **WPF's first-paint cost is non-trivial and unpredictable** because of GC
   and the dispatcher. Cold sessions are far worse than warm.

Both are fixable.

## The four wins (in priority order)

### Win #1 — UIA cache request (saves ~150 ms)

`IUIAutomationCacheRequest` is the single most important API in this project.
It tells UIA: *"when you walk the tree for me, fetch all of these properties
and patterns in one cross-process call. Hand me back a tree where every
element is already cached."*

Pseudo-Rust:

```rust
let cr = automation.CreateCacheRequest()?;
cr.AddProperty(UIA_BoundingRectanglePropertyId)?;
cr.AddProperty(UIA_IsEnabledPropertyId)?;
cr.AddProperty(UIA_IsOffscreenPropertyId)?;
cr.AddProperty(UIA_ControlTypePropertyId)?;
cr.AddProperty(UIA_ClassNamePropertyId)?;
cr.AddPattern(UIA_InvokePatternId)?;
cr.AddPattern(UIA_TogglePatternId)?;
cr.AddPattern(UIA_SelectionItemPatternId)?;
cr.AddPattern(UIA_ExpandCollapsePatternId)?;
cr.AddPattern(UIA_ValuePatternId)?;
cr.SetAutomationElementMode(AutomationElementMode_None)?;     // We don't need live elements; we want cached.

let root_cached = root.BuildUpdatedCache(&cr)?;
// Walk root_cached using TreeWalker on the *cached* view — every property
// access is a local memory read, not a COM call.
```

Two cache request rules:

- **Build once at startup.** A `IUIAutomationCacheRequest` is itself a COM
  object; reuse it across every enumeration.
- **`AutomationElementMode_None`.** This tells UIA we're fine with cached-only
  elements. Saves another round-trip per element.

### Win #2 — `BuildUpdatedCache` over `FindAll` (saves ~30 ms)

`FindAll` returns a flat array, but it does so by recursing in the target
process and calling property-fetch RPCs along the way for each element it
considers. We instead get the *cached subtree root* with `BuildUpdatedCache`
and walk it locally.

Decision rule:

- Tree size estimated < 64 elements → `FindAll` is fine and slightly simpler.
- Tree size estimated ≥ 64 → use `BuildUpdatedCache` + local TreeWalker.

We don't know the tree size in advance. Solution: always use the cached path.
The constant-factor cost on tiny trees is negligible (~0.3 ms) and the upside
is enormous on large ones.

### Win #3 — Direct2D + DirectComposition over WPF (saves ~40 ms)

WPF is brilliant for desktop apps. It's the wrong tool for an overlay that
appears for 200 ms and disappears. Native cost:

| Path                  | Cold paint | Warm paint | Notes                                |
|-----------------------|-----------:|-----------:|--------------------------------------|
| WPF (legacy HAP)      |     55 ms  |     18 ms  | Dispatcher, layout, software raster. |
| Direct2D + DComp      |      6 ms  |      1.5 ms| GPU composited, no GDI redirection.  |

DirectComposition gives us GPU-side, vsync-aligned compositing without
needing to manage a swap chain present cadence. The DWM does the work; we
hand it a tree of visuals.

### Win #4 — Pre-warm at boot (saves ~20 ms cold-start, every cold session)

The first hotkey of the day is otherwise miserable: COM apartments
initialize, UIA's MTA proxy spins up in the target, the swap chain
allocates, brushes are created. We move all of that to startup.

Boot-time work (acceptable since users only pay it once):

- `CoInitializeEx` on input thread (STA), worker pool (MTA), render thread
  (STA).
- Instantiate `CUIAutomation`. Build cache request.
- Create per-monitor overlay windows, hidden.
- Create `ID3D11Device` (BGRA, no debug), `IDXGIFactory2`, swap chain
  template.
- Create `ID2D1Factory1`, `ID2D1DeviceContext`, default brushes,
  `IDWriteFactory`, default `IDWriteTextFormat`.
- Allocate 4 KB hint scratch buffer, 64 entry hint pool.

Target: end-to-end cold start ≤ 150 ms, of which < 50 ms is our code.

## The five smaller wins

### Smaller win #1 — Avoid heap churn in the hot path

In the session loop we never allocate during a keystroke:

- `Vec<&Hint>` for filter results uses a `SmallVec<[&Hint; 64]>`.
- `Hint::label` is a `Box<str>` allocated once at planning time, never
  resized.
- Render scene is double-buffered with two pre-allocated quad arrays.

CI assertion: `nav-bench/filter` measures ≤ 0 allocations per filter call
on 1024 hints (via `cargo build --emit=metadata` + a hand-rolled
`GlobalAlloc` count). If allocations appear, the bench fails.

### Smaller win #2 — Tight data layout

`RawHint` is 40 bytes. `Hint` is 56 bytes. Both fit in single cache lines.
`Vec<Hint>` traversal is linear and prefetcher-friendly. Avoid pointer
indirection — `name` is `Option<Box<str>>` only because *most* hints don't
need it; when present, name comparison happens off the hot path (during
ranking, not filtering).

### Smaller win #3 — Avoid stringy work on each keystroke

Filtering by prefix on `Box<str>` is fine because hint labels are 1–4 chars.
But matching is **case-insensitive ASCII over the alphabet**. We use a
precomputed `u8` byte for each hint and compare bytes:

```rust
struct PackedLabel([u8; 4]);   // ASCII upper, NUL-padded.
fn matches_prefix(label: PackedLabel, prefix: PackedLabel, len: u8) -> bool {
    let mask = (!0u32).wrapping_shr(((4 - len) * 8) as u32);
    let l = u32::from_le_bytes(label.0);
    let p = u32::from_le_bytes(prefix.0);
    (l & mask) == (p & mask)
}
```

Two cmp ops per hint, no branches in the loop body. SSE makes 8x of these
per cycle.

### Smaller win #4 — DPI computed once per session

Per-monitor DPI is a `f32` we capture at session start and apply uniformly.
We never call `GetDpiForWindow` per hint; UIA returns physical pixels and
we render in physical pixels (we declared `PerMonitorV2` in the manifest).

### Smaller win #5 — Avoid waking the UI thread for nothing

The render thread sleeps on a `crossbeam_channel::Receiver`. The input
thread sleeps on `GetMessageW`. The orchestrator owns the main thread
which sleeps on a channel. **No polling. No timers.** Idle CPU stays at
0%.

## Performance gates (CI-enforced)

`12-benchmarking.md` is the canonical document; the gates summary:

| Gate                                          | Threshold (P95)        | Where measured             |
|-----------------------------------------------|------------------------|----------------------------|
| `nav-bench label`                             | ≤ 250 ns / 1024 hints  | Linux + Windows CI         |
| `nav-bench filter`                            | ≤ 1500 ns / 1024 hints | Linux + Windows CI         |
| `nav-bench enumerate_synthetic`               | ≤ 200 µs / 1024 hints  | Linux + Windows CI         |
| `nav-bench enumerate_real (Notepad)`          | ≤ 6 ms                 | Windows CI (manual / cron) |
| `nav-bench enumerate_real (File Explorer)`    | ≤ 15 ms                | Windows CI                 |
| `nav-bench enumerate_real (VS Code)`          | ≤ 22 ms                | Windows CI                 |
| End-to-end harness P95 (reference set)        | ≤ 30 ms                | Manual, recorded weekly    |

Regressions > 5% require an explicit waiver in the PR description, signed
off by another reviewer.

## Anti-optimizations to refuse

These look attractive, are not:

- **Async/await for COM.** COM RPC is blocking; an executor over it is just
  more layers and worse stack traces. Use threads.
- **A scripting layer.** Lua, JS, Rhai — nope. Not in v1, possibly never.
- **Caching the *element tree* across sessions.** UI mutates constantly;
  stale caches mean wrong hints. Cache the *request* object only.
- **Custom font rendering.** DirectWrite is fast and correct. Reinventing
  it costs months for no measurable win.
- **Lock-free everything.** A `parking_lot::Mutex` is ~25 ns uncontended;
  the renderer state is touched only by the render thread; no need for
  exotic structures.
- **Tiny binary heroics.** `#![no_std]`, hand-rolled allocators,
  `compiler_builtins` swaps. Save ~200 KB, lose weeks. Refuse.

## When to profile

After every step in Phase D and Phase F:

1. `tools/flame.ps1` → flame graph.
2. `tools/trace-uia.ps1` → ETW capture filtered to UIA RPC events.
3. `Trace Analyzer` (Windows Performance Analyzer) for the rare GC /
   compositor anomaly.

Profiling is mandatory before any "I made it faster" claim. The bench
numbers are the receipts.
