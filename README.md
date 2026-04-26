# Navigator

> Press a hotkey. Hints appear. Type letters. Act.
> A keyboard-native UI navigator for Windows.

## Status

Pre-alpha. See [Agent/workflow/10-milestones.md](Agent/workflow/10-milestones.md)
for progress.

## Quick start (developers)

```powershell
git clone <this repository URL>
cd <repository-directory>
rustup show  # ensure stable ≥ 1.85 (Rust 2024 edition)
git config core.hooksPath tools/git-hooks
cargo check --workspace
```

The shipping binary will be `nav-app.exe` once the `nav-app` crate exists (M1+).
The legacy Hunt-and-Peck (HAP) C# sources exist only under [`legacy/`](legacy/).

Then press `Alt+;` over any focused window (after `nav-app` ships).

## Documentation

- **End users:** see `docs/` (post-v1).
- **Contributors:** start at [Agent/workflow/README.md](Agent/workflow/README.md).
- **Legacy C# implementation:** see [`legacy/`](legacy/). Read-only.

## License

MIT OR Apache-2.0. The legacy code retains its original license inside
`legacy/`.
