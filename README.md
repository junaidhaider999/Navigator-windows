# Navigator v1.0

Press a hotkey. Hints appear. Type letters. Act.

A keyboard-native UI navigator for **Windows**, rewritten in Rust.

## Screenshots

| Notepad | File Explorer |
| :---: | :---: |
| ![Navigator hints in Notepad](public/screenshots/notepad.png) | ![Navigator hints in Explorer](public/screenshots/explorer.png) |

## Credits

Navigator follows the **Hunt-and-Peck** idea from **[zsims](https://github.com/zsims)**’s original project: **[hunt-and-peck](https://github.com/zsims/hunt-and-peck)**. This repo is a **Rust rewrite** of that workflow (see `crates/`); it is not line-by-line ported C#.

Thank you, **zsims**, for the concept and the reference implementation.

## Usage

1. Run **`navigator.exe`** (see **Build**).
2. Focus a normal window (e.g. Notepad or Explorer).
3. Press **`Alt+/`** to open hints everywhere. You can also use plain **`/`** when focus is not in a text field (see in-app behavior).
4. Type the shown letters to filter, then activate the target.

**Esc** or pressing the global hotkey again cancels.

## Build

- **OS:** Windows  
- **Rust:** ≥ 1.85 (`edition = "2024"`)

```powershell
cargo build -p nav-app --release
```

Output: `target/release/navigator.exe`.

Developers:

```powershell
git clone <repository-url>
cd navigator
cargo check --workspace
cargo run -p nav-app --bin navigator
```

## Docs

| Audience | Where |
|----------|--------|
| Contributors | [Agent/workflow/README.md](Agent/workflow/README.md) |
| Legacy C# HAP (read-only) | [legacy/](legacy/) |

## License

This Rust workspace is licensed under the **GNU General Public License v3.0 only** — see [`LICENSE`](LICENSE).

The archived Hunt-and-Peck sources under [`legacy/`](legacy/) keep their **original license** as shipped upstream.
