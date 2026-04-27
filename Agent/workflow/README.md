# Navigator — Execution Workflow

> Next-generation rewrite of Hunt-and-Peck. Press a hotkey, see hints, type letters,
> act. Anywhere on Windows. Faster than blinking.

This folder is the **single source of truth** for how Navigator gets built. It is
written for engineers and AI agents working on the project. Read top-to-bottom on
your first pass, then use it as a reference.

---

## How to use this workflow

1. **Start at `00-overview.md`.** Internalize the principles. Every later
   decision is downstream of them.
2. **Read in order on first pass.** Each document assumes the previous ones.
3. **Treat the performance budget as a contract.** Every PR must justify any
   regression in the numbers tracked in `12-benchmarking.md`.
4. **Use `04-build-order.md` as your daily checklist.** Do not skip ahead.
5. **When in doubt, default to: simpler, smaller, faster.** If a feature
   threatens speed/simplicity/reliability, reject it.

---

## Implementation snapshot (sync with code)

On the [status board in `10-milestones.md`](10-milestones.md#status-board-live):
**M0–M5** are **Done** (or **Done*** where noted); **M6** (UIA cache / D1) and **M7**
(overlay pre-warm / D2) are **Done** in code. Phase **D** items **D3** (parallel HWND
subtrees) and **D4** (partial overlay repaint) are also in-tree — see
[`04-build-order.md`](04-build-order.md) Phase D **Implemented** bullets. Phase D
**exit** metrics (P95 on the reference set) remain to be proven on the reference
machine.

Phase **E** — **E1** (MSAA) and **E2** (raw HWND) fallback enumerators, `Auto` ladder,
per-step budgets, and tray **Diagnose** are **in tree**; see
[`m9-acceptance.md`](m9-acceptance.md) for acceptance tests and what remains manual
(coverage %, 1000-trigger field runs). **E3–E5** details: `04-build-order.md` Phase E;
release packaging is still **M11**.

**Configuration** — `nav-config`: `[hints]`, `[log]`, `[fallback.budget_ms]`;
`load_for_startup` discovery; **`--reset-config`**; tray **Reload** reapplies
hints and budgets. Full schema, appearance hot reload, and file watcher remain
in [`13-configuration.md`](13-configuration.md) (partial M10).

---

## Index

| #  | File                                | Purpose                                                                         |
|----|-------------------------------------|---------------------------------------------------------------------------------|
| M9 | `m9-acceptance.md`                  | M9 fallback ladder, budget parity, CI vs manual gates.                            |
| 00 | `00-overview.md`                    | Vision, principles, hard performance budget, non-goals.                         |
| 01 | `01-architecture.md`                | System architecture, threading model, data flow, control flow.                  |
| 02 | `02-folder-structure.md`            | Repo layout, Cargo workspace, crate boundaries.                                 |
| 03 | `03-modules.md`                     | Module-by-module responsibilities and public surface.                           |
| 04 | `04-build-order.md`                 | Strict ordered build sequence — what to implement first and why.                |
| 05 | `05-performance-strategy.md`        | Where the milliseconds go and how to claw them back.                            |
| 06 | `06-windows-apis.md`                | Win32 API choices and the UIA → MSAA → raw HWND fallback ladder.                |
| 07 | `07-rendering-strategy.md`          | Layered window + DirectComposition + Direct2D overlay pipeline.                 |
| 08 | `08-hint-generation.md`             | Label alphabet, distribution algorithm, filtering, ranking.                     |
| 09 | `09-input-handling.md`              | Hotkey path, low-level keyboard hook, state machine, key swallowing.            |
| 10 | `10-milestones.md`                  | M0 → M11 roadmap from prototype to "elite" version.                             |
| 11 | `11-legacy-migration.md`            | How to move the old C# HAP code into `/legacy` cleanly.                         |
| 12 | `12-benchmarking.md`                | What we measure, how we measure it, and the regression gate.                    |
| 13 | `13-configuration.md`               | Config file format, CLI flags, AutoHotkey integration surface.                  |
| 14 | `14-risks-and-decisions.md`         | Architectural Decision Records (ADRs) and known risks.                          |

---

## Project tagline

> **Navigator is not a feature platform. Navigator is a 16ms reflex.**

Anything that does not serve that reflex is out of scope for v1.

---

## Glossary

- **HAP** — Hunt-and-Peck, the legacy C#/WPF tool this project replaces.
- **Hint** — A short label (e.g. `SA`, `JK`) overlaid on a clickable element.
- **Hint session** — One full lifecycle: hotkey → enumerate → render → input → act.
- **UIA** — UI Automation, the modern Windows accessibility API.
- **MSAA** — Microsoft Active Accessibility, the legacy accessibility API.
- **Hotkey latency** — Time from physical key press to first visible hint pixel.
- **Elite target** — Sub-16ms hotkey latency; the gold standard.
- **Standard target** — Sub-30ms hotkey latency; the minimum acceptable.

---

## Working agreement

- No premature complexity. **Prototype → benchmark → optimize.** Never the other order.
- No new dependencies without a written reason in `14-risks-and-decisions.md`.
- No regression in P50/P95 latency without an explicit, accepted trade-off.
- Legacy code in `/legacy` is **read-only** reference material. Do not patch it.
- Every public API in `nav-core` must be testable without Windows.

---
