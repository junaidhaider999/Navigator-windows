# Navigator

> Press a hotkey. Hints appear. Type letters. Act.
> A keyboard-native UI navigator for Windows.

## Status

**Phase C (MVP path) is implemented:** `Alt+;` → UIA enumerate → `nav-core` plan
→ session → overlay pills → type labels → UIA invoke; Esc and second hotkey
cancel cleanly. Enumeration uses a UIA cache (D1) but can still be slow on very
large trees; Phase D continues with pre-warm (D2) and further optimization. See
[Agent/workflow/10-milestones.md](Agent/workflow/10-milestones.md) for the
milestone table.

## Quick start (developers)

```powershell
git clone <this repository URL>
cd <repository-directory>
rustup show  # ensure stable ≥ 1.85 (Rust 2024 edition)
git config core.hooksPath tools/git-hooks
cargo check --workspace
# Windows — run the navigator (requires focused HWND, e.g. Notepad):
cargo run -p nav-app --bin navigator
```

The pre-commit hook refuses any commit that stages changes under `legacy/`
(see `tools/git-hooks/pre-commit`). Replay or bulk edits that touch `legacy/`
must use `git commit --no-verify` only when intentional, per
`Agent/workflow/11-legacy-migration.md`.

The shipping binary is built from crate **`nav-app`** as **`navigator`**
(`cargo build -p nav-app` → `target/<profile>/navigator.exe` on Windows).
Pure logic lives in `crates/nav-core` (cross-platform); Criterion benches in `crates/nav-bench`.

The legacy Hunt-and-Peck (HAP) C# sources exist only under [`legacy/`](legacy/).

Then focus a window (e.g. Notepad) and press **`Alt+;`** to start a hint session.

## Documentation

- **End users:** see `docs/` (post-v1).
- **Contributors:** start at [Agent/workflow/README.md](Agent/workflow/README.md).
- **Legacy C# implementation:** see [`legacy/`](legacy/). Read-only.

## License

MIT OR Apache-2.0. The legacy code retains its original license inside
`legacy/`.
