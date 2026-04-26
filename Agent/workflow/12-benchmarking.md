# 12 — Benchmarking

> Performance claims without numbers are folklore. This document is the
> measurement contract: what we measure, how, and what counts as a pass.

## Three levels of benchmarks

### Level 1: micro (`cargo bench`, every CI run)

Pure-Rust criterion benches that run in seconds and have no Windows
dependency.

| Bench                         | What it measures                                  | Gate (P95)                |
|-------------------------------|---------------------------------------------------|---------------------------|
| `nav-bench label`             | `generate_labels(N)` for N ∈ {14, 100, 1024, 5000}| ≤ 250 ns / 1024 hints     |
| `nav-bench filter`            | Prefix filter over 1024 hints, varying prefix len | ≤ 1500 ns / 1024 hints    |
| `nav-bench session`           | `Session::key` hot path                            | ≤ 800 ns                  |
| `nav-bench planner`           | Planner ranking + label assignment, N=1024        | ≤ 12 µs                   |
| `nav-bench enumerate_synth`   | Synthetic UIA tree walker, N=1024 (mock COM)      | ≤ 200 µs                  |

These run on **both** Linux and Windows CI. They form the default
regression gate that blocks merges.

### Level 2: macro (`nav-bench enumerate_real`, manual + nightly)

End-to-end synthetic-app benchmarks that actually drive the UIA pipeline
against fixture binaries we ship.

| Bench fixture                      | Approx elements | Gate (P95)        |
|------------------------------------|-----------------|-------------------|
| `nav-fixture/notepad-clone`        |  ~12            | ≤  6 ms           |
| `nav-fixture/file-explorer-mock`   |  ~85            | ≤ 15 ms           |
| `nav-fixture/vscode-like`          |  ~280           | ≤ 22 ms           |
| `nav-fixture/giant-tree`           | 1024            | ≤ 35 ms           |

Fixture apps live under `tests/fixtures/`. They are minimal Win32 / WPF
demos with known element counts. Real-world apps (real Notepad, real VS
Code) are tested in Level 3.

### Level 3: real-world matrix (manual, weekly)

A spreadsheet of real apps tested by hand on a reference machine. Used to
catch coverage / reliability regressions, not measure latency.

| App                         | Coverage target | Hint-to-action target |
|-----------------------------|-----------------|-----------------------|
| Windows File Explorer       | ≥ 95 %          | works                 |
| Notepad                     | ≥ 99 %          | works                 |
| Visual Studio Code          | ≥ 90 %          | works                 |
| Visual Studio 2022          | ≥ 90 %          | works                 |
| Chrome (current GA)         | ≥ 90 %          | works                 |
| Office Word (M365)          | ≥ 85 %          | works                 |
| Slack (Electron)            | ≥ 85 %          | works                 |
| Settings (UWP)              | ≥ 85 %          | works                 |
| Win32 "Run" dialog (`Win+R`)| ≥ 95 % (MSAA)   | works                 |

If a row drops below target, file an issue tagged `coverage-regression`.

## The reference machine

We define **one** machine spec for canonical numbers:

```
CPU:      Intel i7-1260P or AMD Ryzen 7 7840U
RAM:      32 GB
Storage:  NVMe SSD
GPU:      Integrated (Iris Xe / Radeon 780M). We measure the worst case.
OS:       Windows 11 24H2, fully updated.
DPI:      150% on a 2560×1600 display.
Power:    "Best performance" plan, A/C plugged in.
Display:  Single monitor.
Bench:    Foreground app idle (no background CPU).
```

If you do not have this hardware exactly, your numbers are **provisional**.
The CI machine on `windows-latest` GitHub-hosted runners is the second
canonical environment; numbers from there are tracked separately.

## Pre-Phase-D baseline (uncached UIA, real apps)

Before **M6 / D1** lands, `nav-uia` uses a slow **`FindAll(TreeScope_Descendants, …)`**
walk without a cache request. On a typical dev machine, **hundreds of milliseconds**
for ~100+ invoke-pattern nodes is expected. Record your machine’s numbers here
when you change enumeration so Phase D can show a clear delta:

| Date       | App / HWND role   | Elements | Wall time (ms) | Notes                          |
|------------|-------------------|----------|----------------|--------------------------------|
| 2026-04-26 | Example (fill in) | —        | —              | Replace row when benching.   |

## How we measure hotkey-to-pixels latency

This is the headline metric. Capturing it correctly is harder than it
sounds because it spans multiple processes and a GPU compositor.

### Method A — Instrumented (default, in CI)

We instrument timestamps at four points and report the deltas:

1. `t0` — `WM_HOTKEY` handler entry. `QueryPerformanceCounter` immediately.
2. `t1` — Enumeration complete (UIA returned). QPC.
3. `t2` — `IDXGISwapChain1::Present` returned. QPC.
4. `t3` — `IDCompositionDevice::Commit` returned. QPC.

We log `(t1 - t0)`, `(t2 - t1)`, `(t3 - t2)`, and the sum.

This *does not* include DWM compose-to-screen latency, which is one to two
vsyncs (8–16 ms at 60 Hz, 4–8 ms at 144 Hz). We approximate that as a
constant offset. For the contract numbers we use the sum from above and
budget for compose separately.

### Method B — High-speed camera (verification, quarterly)

Run the binary on the reference machine, point a 240 fps camera at the
screen and the keyboard. Press `Alt+;`. Frame-step until you see hints
appear. Compute `(hint_frame - keypress_frame) * (1000 / 240)` ms.

This is the **truth**. Method A's instrumented numbers must agree with
Method B's camera numbers within ±5 ms. If they don't, our instrumentation
is lying.

We run Method B once per quarter, after every major refactor of the
input/render path, and before any release. Results go into the release
notes.

### Method C — `keybd_event` synthetic harness (smoke test)

For automation, the harness spawns the binary, waits for "ready" log line,
then injects `Alt+;` via `keybd_event` and reads the timestamps from the
log. Less accurate than Method A (the OS injection adds 1–3 ms variance)
but lets us run 1000 trials in a minute.

This runs on Windows CI as a smoke test. Failure threshold: P95 sum of
the four deltas above ≤ 30 ms (standard) or ≤ 16 ms (elite, post-M12).

## What's in a "P95"

We collect at least **1000 samples** per measurement. We report:

- P50 (median) — typical user experience.
- P95 — the bar we hold the line on.
- P99 — caught-on-tape worst case.
- Min, Max — sanity checks.

We do **not** report mean. Means hide tail latency, which is exactly what
matters here.

The harness uses `criterion` for Level 1 and a hand-rolled measurement
loop for Levels 2 & 3. Both write JSON to `target/bench-results/`.

## Regression policy

1. Every PR that touches `nav-uia`, `nav-render`, `nav-input`, or
   `nav-core` must run Level 1 benches and post the diff in the PR
   description.
2. CI automatically diffs against the `main` baseline using
   `criterion-compare` (or our own diff script).
3. Any P95 regression > 5 % blocks merge unless the PR description
   explicitly accepts the trade-off and a maintainer sign-off agrees.
4. Level 2 benches run nightly. A regression there opens a P0 issue.

## The "no surprise allocations" gate

Some of the hot paths must not allocate. We enforce this with a dhat
(`#[cfg(test)]`) integration inside `nav-bench`:

```rust
#[test]
fn filter_does_not_allocate() {
    let hints = sample_hints(1024);
    let session = Session::with_hints(hints);

    let stats0 = dhat::HeapStats::get();
    for c in "fjkw".chars() {
        let _ = session.key(c);
    }
    let stats1 = dhat::HeapStats::get();

    assert_eq!(stats1.total_blocks - stats0.total_blocks, 0);
}
```

Hot paths protected by this gate:

- `Session::key` (after warm-up).
- `nav-render::Renderer::update` (steady state).
- `nav-input::ll_kbd` callback (always — it has no business allocating).

## Profiling tooling

| Tool                        | Use case                                                          |
|-----------------------------|-------------------------------------------------------------------|
| `cargo flamegraph`          | Linux-side micro analysis of `nav-core`.                          |
| Windows Performance Recorder| Full-system ETW capture during enumeration.                       |
| Windows Performance Analyzer| Reading WPR `.etl` files; UIA RPC events live under `Microsoft-Windows-UIAutomationCore`. |
| PIX on Windows              | GPU frame analysis of D2D/DComp pipeline.                         |
| `tools/flame.ps1`           | Wraps WPR into a single-command capture, produces a flame graph.  |
| `tools/trace-uia.ps1`       | WPR profile filtered to UIA-relevant providers.                   |

`tools/flame.ps1` is committed to the repo. It is the answer to "why is
this slow" for any Windows-side hot path.

## Reporting numbers in PRs

The expected format:

```
### Bench impact (vs main @ <sha>)

| Bench                        | Before P95 | After P95 | Δ      |
|------------------------------|-----------:|----------:|-------:|
| nav-bench label              | 240 ns     | 235 ns    | -2.1 % |
| nav-bench filter             | 1.46 µs    | 1.43 µs   | -2.0 % |
| enumerate_real::vscode_like  | 21.3 ms    | 18.7 ms   | -12.2 %|

Method: criterion 30 samples, win-bench-runner.ps1 100 trials, ref machine.
```

If a regression is intentional, explain *why*, what we got back, and the
sunset condition for the regression.

## Numbers to write down today

When M3 lands, fill this table with **your real measured numbers**. They
are the floor that Phase D must improve on.

```
=== M3 baseline (no caching) ===
machine: <hostname>, <CPU>, <RAM>, <OS build>
date:    <YYYY-MM-DD>

Notepad        :  P50 __ ms  P95 __ ms  N=1000
File Explorer  :  P50 __ ms  P95 __ ms  N=1000
VS Code        :  P50 __ ms  P95 __ ms  N=1000
Visual Studio  :  P50 __ ms  P95 __ ms  N=1000
```

When M11 lands, the same table is filled with the v1 release numbers and
embedded in the README.
