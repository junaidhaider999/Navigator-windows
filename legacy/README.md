# Legacy Hunt-and-Peck (HAP) — Read-only Reference

This folder contains the original C# WPF implementation of Hunt-and-Peck.
**Do not modify it.** Navigator (the new Rust implementation in `crates/`)
is independent and supersedes it.

## Why we kept it

- Reference for behaviors we want to preserve (hint algorithm, `Alt+;`
  default, supported pattern set).
- Comparable performance baseline — when we say "X is 10× faster than HAP",
  we mean *this* HAP.
- Pointer to history: this is what the project was when it landed here.

## What you can do here

- `git log --follow legacy/<path>` to read history.
- Open `.csproj` files in Visual Studio if you want to run the old version.
  You will need .NET Framework 4.6.2 / 4.7.2 and the Windows 10 SDK.
- Cherry-pick algorithm details for `nav-core::label` and friends. The
  hint label generator in `legacy/src/HuntAndPeck/Services/HintLabelService.cs`
  is canonical for vimium-style distribution.

## What you must not do

- Do **not** add new features here.
- Do **not** fix bugs here.
- Do **not** import this code into the new build.
- Do **not** target it from CI; CI ignores `legacy/`.

If a bug in HAP must be fixed for backward compatibility (extremely
unlikely), open an issue, then redirect to fixing the equivalent in
`nav-*` crates.

## When this folder gets deleted

Once Navigator v1.0 is shipped, has parity-or-better coverage on the
fixture matrix, and at least 30 days of production use show no need for
HAP-side reference, this folder may be deleted in a clearly-tagged commit
(`legacy: remove HAP after v1 parity reached`).

Until then, it stays. Disk is cheap; institutional memory is not.
