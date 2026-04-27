# 00 — Overview

## What Navigator is

Navigator is a **keyboard-driven UI hint navigator for Windows**. Press a global
hotkey, hints appear over every clickable element in the focused window, type the
letters of a hint, the action fires. That is the entire product.

It is a spiritual successor to Hunt-and-Peck (HAP) and to Vimium-for-the-OS, but
rewritten from first principles to be:

1. **Blazing fast** — sub-30ms typical, sub-16ms elite, hotkey-to-pixels.
2. **Minimal** — single binary, single config file, zero install dance.
3. **Reliable** — when standard accessibility fails, fall back, never disappear.
4. **Keyboard-native** — never requires the mouse, ever.
5. **Modern Windows compatible** — Win10 1809+ and Win11 across Win32, WPF,
   WinForms, Electron, Chromium, mixed legacy stacks.

## What Navigator is not

These are **explicitly out of scope** for v1. Reject any PR that adds them:

- ❌ Themes / theme engine / CSS-like styling
- ❌ Animations, easing, fades (one fixed snap-on, snap-off)
- ❌ Plugin system / scripting host
- ❌ Macro recorder / chain runner
- ❌ Cloud sync / accounts / telemetry
- ❌ AI-powered "smart suggestions"
- ❌ Settings GUI with tabs and panels (config file + tray "open config" is enough)
- ❌ Cross-platform support (Windows-only by design)
- ❌ Mouse mode / drag mode / scroll mode (later, never v1)

If a request falls into one of these buckets, the answer is **no**, ship v1 first.

## Principles, in priority order

When two principles conflict, the **higher-numbered one loses**.

1. **Latency is the product.** A 200ms hint engine is a different product from a
   16ms hint engine. We ship the 16ms one.
2. **Reliability beats coverage.** Better to find 90% of clickable elements
   reliably in 10ms than 99% unreliably in 100ms.
3. **Simplicity beats configurability.** Every option is a maintenance tax and a
   user decision. Prefer one good default.
4. **Keyboard-only or it didn't ship.** No flow may require the mouse.
5. **Single binary, single process.** No services, no helpers, no daemons,
   unless profiling proves we need them.
6. **Measure, don't guess.** Performance claims without `cargo bench` numbers
   are rumors.
7. **Native over abstract.** Win32/COM directly. No Electron, no WPF, no
   .NET runtime in the hot path.
8. **Cold start matters.** First trigger after launch must be as fast as the
   thousandth. Pre-warm everything.
9. **No surprise CPU.** Idle CPU must be ~0%. We are a reflex, not a service.
10. **Forward-compatible, not backward-compatible.** Win10 1809+ floor is fine.
    Pre-1809 is the legacy HAP's job.

## The performance budget

The numbers below are **contracts**, not aspirations. They are the gate every
build passes through (`12-benchmarking.md` defines the harness).

### Hotkey-to-pixels latency, P50 / P95

Measured from the kernel-injected key event to the first hint pixel composited
on screen, on a typical reference window (~150 actionable elements).

| Stage                                     | Standard budget | Elite budget |
|-------------------------------------------|-----------------|--------------|
| Hotkey dispatch (WM_HOTKEY → handler)     |   1.0 ms        |   0.3 ms     |
| Foreground HWND + window rect             |   0.2 ms        |   0.1 ms     |
| UIA enumeration (cached, batched)         |  15.0 ms        |   8.0 ms     |
| Coordinate transform + filter             |   1.0 ms        |   0.5 ms     |
| Hint label assignment                     |   0.5 ms        |   0.2 ms     |
| Direct2D draw command list                |   3.0 ms        |   1.5 ms     |
| DirectComposition commit + present        |   8.0 ms        |   4.0 ms     |
| Slack                                     |   1.3 ms        |   1.4 ms     |
| **Total**                                 | **30 ms**       | **16 ms**    |

### Resource budget

| Metric                        | Limit         |
|-------------------------------|---------------|
| Idle RSS                      | ≤ 25 MB       |
| Active RSS during hint mode   | ≤ 60 MB       |
| Idle CPU                      | < 0.1 %       |
| Cold-start to ready           | ≤ 150 ms      |
| Single binary size (release)  | ≤ 4 MB        |
| Disk install footprint        | ≤ 8 MB        |

### Reliability budget

| Metric                                     | Target              |
|--------------------------------------------|---------------------|
| Hotkey-to-hint success rate                | ≥ 99.9 % over 1000 triggers |
| Crash rate                                 | 0 in normal use     |
| Stuck-overlay rate (hint mode never exits) | 0                   |
| Element coverage on Win32 apps             | ≥ 95 %              |
| Element coverage on WPF/WinForms           | ≥ 98 %              |
| Element coverage on Chromium/Electron      | ≥ 90 %              |
| Element coverage on UWP/WinUI 3            | ≥ 85 %              |

## Target user

A power user who:

- Lives in keyboard mode for hours a day.
- Triggers Navigator dozens of times per hour.
- Will *feel* a 50ms regression and complain about it.
- Will replace Navigator with a competitor the moment it stutters.

Design for this user. Everything else follows.

## Success definition for v1

> A v1 release is shippable when, on a stock Win11 24H2 laptop, ten consecutive
> hint sessions across File Explorer, Visual Studio Code, Chrome, and Notepad
> all complete with **P95 hotkey-to-pixels under 30 ms**, no missed actionable
> elements on the reference set, no stuck overlays, no flicker, no crashes,
> across two different DPI scalings and a multi-monitor setup.

That is the entire bar. Hit it, ship it, then we earn the right to discuss v2.

## Current implementation vs this document

The **Rust workspace** in `crates/` delivers **Phase C** (hotkey → enumerate →
plan → overlay → typed invoke) plus **much of Phase D** in code: D1 UIA cache
request + `FindAllBuildCache` (with `FindAll` fallback on some providers), D2
render prewarm, D3 Rayon HWND subtree walks when the cached tree is large
enough, D4 dirty-region pill repaints. **P95 hotkey-to-hint under 30 ms** on the
reference set is still **not claimed** — see `12-benchmarking.md` and
`10-milestones.md`. Treat the latency targets in this doc as the **contract to
converge on**, not a statement about every HWND today.
