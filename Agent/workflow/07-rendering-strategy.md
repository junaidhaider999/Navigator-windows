# 07 — Rendering Strategy

> The hint overlay is the user's only visual contact with Navigator. It must
> appear instantly, never flicker, never steal focus, never cost the user a
> frame on the underlying app.

## Decision: layered windows + DirectComposition + Direct2D

We render hints into one **layered window per monitor**, composed by DWM via
DirectComposition, drawn with Direct2D. No GDI, no WPF, no XAML, no software
rasterizer.

### Why each piece

- **Layered window (`WS_EX_LAYERED`)** — the only way Windows allows true
  per-pixel alpha + click-through behavior we need.
- **`WS_EX_NOREDIRECTIONBITMAP`** — we are not a GDI window. Skipping the
  redirection bitmap saves a per-frame copy and lets DComp own the surface.
- **DirectComposition** — gives us a GPU-side visual tree composited by the
  DWM at vsync. No `Present` cadence to manage. Smooth, flicker-free.
- **Direct2D** — hardware-accelerated 2D primitives (rounded rects, text via
  DirectWrite). Tiny and fast.
- **DirectWrite** — correct, fast, ClearType-aware text. Reinventing it would
  be insane.

### What this rejects

- ❌ WPF (cold paint cost, GC, dispatcher).
- ❌ Win2D (managed wrapper; we are unmanaged).
- ❌ Skia (great, but yet another dependency we don't need; D2D is shipped
  with the OS).
- ❌ wgpu (we don't need cross-platform; Win32 native is faster).
- ❌ GDI (no hardware accel, flicker, redirection conflicts with layered).

## Window properties (each overlay window)

```text
className   : "Navigator.Overlay"
windowName  : "" (empty)
style       : WS_POPUP
exStyle     : WS_EX_LAYERED
              | WS_EX_TRANSPARENT     // input passes through
              | WS_EX_TOPMOST         // always above the target
              | WS_EX_NOACTIVATE      // never gets focus
              | WS_EX_TOOLWINDOW      // not in Alt+Tab, not in taskbar
              | WS_EX_NOREDIRECTIONBITMAP // DComp-only surface
parent      : None (top-level)
position    : One window per monitor, sized to that monitor's bounds.
```

The overlay is **created once at boot, hidden** with `ShowWindow(SW_HIDE)`,
and shown via `SetWindowPos(... | SWP_NOACTIVATE | SWP_SHOWWINDOW)` per
session. We never destroy and re-create it.

## Pipeline

```
                        ┌─────────────────────────────────────────┐
                        │             Render thread               │
   render.show(hints) ─▶│                                         │
                        │  ┌────────────────────────────────────┐ │
                        │  │  Per-monitor scene encoder         │ │
                        │  │  • Filter hints by monitor         │ │
                        │  │  • Build draw list                 │ │
                        │  │  • Mark damaged regions            │ │
                        │  └─────────────┬──────────────────────┘ │
                        │                ▼                        │
                        │  ┌────────────────────────────────────┐ │
                        │  │  Direct2D batch                    │ │
                        │  │  • BeginDraw → clear (transparent) │ │
                        │  │  • For each hint: rounded rect +   │ │
                        │  │    DrawText (or atlas quad in F1)  │ │
                        │  │  • EndDraw                         │ │
                        │  └─────────────┬──────────────────────┘ │
                        │                ▼                        │
                        │  ┌────────────────────────────────────┐ │
                        │  │  DXGI present + DComp commit       │ │
                        │  │  • IDXGISwapChain1::Present(0,0)   │ │
                        │  │  • IDCompositionDevice::Commit     │ │
                        │  └────────────────────────────────────┘ │
                        └─────────────────────────────────────────┘
```

## Resource lifetimes

| Resource                                | Created                  | Destroyed         |
|-----------------------------------------|--------------------------|-------------------|
| `ID3D11Device` (BGRA, no debug)         | Once at boot             | App exit          |
| `IDXGIFactory2`                         | Once at boot             | App exit          |
| `ID2D1Factory1`                         | Once at boot             | App exit          |
| `IDWriteFactory`                        | Once at boot             | App exit          |
| `IDWriteTextFormat` (default size)      | Once per config reload   | Replaced          |
| Per-monitor swap chain                  | Boot, sized to monitor   | App exit          |
| Per-monitor `IDCompositionVisual`       | Boot                     | App exit          |
| Per-monitor `ID2D1DeviceContext`        | Boot                     | App exit          |
| Per-monitor `ID2D1Bitmap1` back surface | Boot, recreated on resize| App exit / DPI    |
| Default brushes (bg, fg, dim, accent)   | Boot                     | Reload / app exit |
| Hint quad scratch buffer                | Boot, 4 KB               | App exit          |

**Reload flow** (when config changes brushes/font):

1. Pause the render thread.
2. Drop and re-create only `IDWriteTextFormat` + brushes.
3. Resume.

We do **not** rebuild the device, swap chain, or visual on a config reload.

## Visual specification

### Hint pill

A small rounded rectangle with the label inside.

| Property                     | Default            | Configurable? |
|------------------------------|--------------------|---------------|
| Corner radius                | 4 px               | Yes           |
| Padding                      | 3 px horizontal    | Yes           |
| Background                   | `#1B1F2EE6` (90% α)| Yes           |
| Border                       | 1 px `#3D7DFF80`  | Yes           |
| Foreground (typed prefix)    | `#3D7DFF`          | Yes           |
| Foreground (remaining label) | `#FFFFFF`          | Yes           |
| Foreground (filtered out)    | `#5C677A`          | Yes           |
| Drop shadow                  | None (perf)        | Off in v1     |

When the user types a prefix, hints split visually:

```
   ┌──┐  ┌──┐  ┌──┐
   │JK│  │SA│  │EW│
   └──┘  └──┘  └──┘
```

After typing `J`:

```
   ┌──┐
   │JK│   ← 'J' rendered in accent, 'K' in fg.
   └──┘
   (others dimmed and pushed to lowest opacity)
```

This split is purely a render-side filter; we do not reflow.

### Anchor strategy

Hints are positioned at the **top-left corner of the element's bounding
rect**, offset by 2 px right and 2 px down so the corner of the rect remains
visible behind the pill.

When two hints would overlap (e.g. an inner button very close to its parent's
edge), the planner pushes the conflicting hint to the next free quadrant of
the parent in the order TL → TR → BL → BR.

### Font

- DirectWrite default: `Segoe UI Variable Display`, weight 600.
- Size: 14 pt (configurable). Sub-pixel positioning enabled.
- ClearType: enabled. Runs through DWM compositor.

### Performance: glyph atlas (F1, post-v1)

The default DirectWrite path is fast enough for v1. For the elite tier:

1. At config-load time, pre-rasterize the alphabet into a single
   `ID2D1Bitmap1` glyph atlas (a 256×64 BGRA texture).
2. At render time, hint labels draw as instanced quads sampling the atlas.
3. Per-hint draw cost drops from ~40 µs to ~1 µs.

This is a **post-v1** optimization. Do not implement until the simple
`DrawText` path is correct and the v1 budget is being hit.

## Multi-monitor

`EnumDisplayMonitors` at boot returns a list of `HMONITOR`. We create one
overlay window per monitor, sized to its `MONITORINFO::rcMonitor`.

DPI handling:

- We declare `PerMonitorV2` in the manifest.
- Each overlay window's swap chain is sized to the **physical** pixel size
  of its monitor.
- Hints arrive from `nav-uia` in **physical** coordinates. We assign a hint
  to a monitor by checking which monitor rect contains the hint's center.
- Drawing is in physical pixels. We do not apply DPI scaling in code.

When a monitor configuration changes (DisplayChange, DPI change, monitor
hot-plug), we receive `WM_DISPLAYCHANGE` / `WM_DPICHANGED_AFTERPARENT` on
the input thread, post a "rebuild monitors" event to the render thread,
which then:

1. Releases all per-monitor swap chains and visuals.
2. Re-enumerates monitors.
3. Re-creates per-monitor resources.
4. Resumes accepting `show()` calls.

## Click-through and focus

- `WS_EX_TRANSPARENT` makes the window invisible to mouse hit-testing.
  Clicks pass through.
- `WS_EX_NOACTIVATE` means showing the window does not steal focus.
- We never call `SetForegroundWindow` on the overlay. Ever.

This is why our LL keyboard hook (in `nav-input`) does the input capture —
the overlay window itself receives no keys.

## Frame timing

- We do **not** use `Present(1, 0)` (vsync wait). We `Present(0, 0)` and
  let DComp do the vsync alignment. This is faster on transitions because
  the first present after a long idle does not block on the previous
  vblank.
- We commit the DComp tree **before** present. The commit is what tells
  DWM we want this frame; present provides the surface.
- Filter updates redraw only the hint quads that changed. The clear is
  per-region (`PushAxisAlignedClip`), not full-screen.

Target render-thread frame budget per `update()`: **≤ 1.5 ms P95**.
Target render-thread frame budget per `show()` (full scene): **≤ 4 ms P95**.

## Failure modes

| Failure                                              | Recovery                                                  |
|------------------------------------------------------|-----------------------------------------------------------|
| `D3D11CreateDevice` fails (rare driver bug)          | Retry once with `D3D_DRIVER_TYPE_WARP`. Log "soft GPU".   |
| `Present` returns `DXGI_ERROR_DEVICE_REMOVED`        | Drop device, re-create, redraw current scene.             |
| Layered window fails to create                       | Log, fall back to no overlay (impossible to recover; surface error to tray and exit). |
| DComp commit returns failure                         | Drop visual, recreate, retry once. Then give up session.  |
| `IDWriteTextLayout::DrawTextLayout` text overflow    | Truncate label to first 4 chars. Should never happen.     |

## What rendering is **not** allowed to do

- ❌ Block on UIA enumeration. `nav-uia` runs on workers.
- ❌ Touch a Win32 hotkey or hook API. `nav-input` owns those.
- ❌ Allocate during `update()`. Quad scratch is preallocated.
- ❌ Redraw at idle. After present, the render thread is parked on its
  channel until the next `show`/`update`/`hide`.
- ❌ Use the dispatcher / a message pump for per-frame work. The render
  thread runs a custom loop driven by the channel.

## Pseudo-API

```rust
// crates/nav-render/src/lib.rs

pub struct Renderer { /* opaque */ }

impl Renderer {
    pub fn spawn(cfg: &Config) -> Result<Self, RenderError> { /* boot */ }

    pub fn show(&self, sid: SessionId, hints: &[Hint]) -> Result<()> {
        self.tx.send(Cmd::Show { sid, hints: hints.into() })?;
        Ok(())
    }

    pub fn update(&self, sid: SessionId, mask: &VisibleMask) -> Result<()> {
        self.tx.send(Cmd::Update { sid, mask })?;
        Ok(())
    }

    pub fn hide(&self, sid: SessionId) -> Result<()> {
        self.tx.send(Cmd::Hide { sid })?;
        Ok(())
    }
}
```

Errors are recoverable; the orchestrator decides whether to retry.
