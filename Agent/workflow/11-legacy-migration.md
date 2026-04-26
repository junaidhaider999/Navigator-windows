# 11 — Legacy Migration

> The old C# Hunt-and-Peck code goes into `/legacy` as **read-only
> reference**. We do not patch it. We do not build it. We mine it for
> insight, then we ignore it.

## What lives in `/legacy`

After this migration, the directory layout is:

```
legacy/
├── README.md                 # Pointer note (this file's runtime artifact).
├── src/
│   ├── HuntAndPeck/          # The original WPF app, verbatim.
│   ├── HuntAndPeck.Tests/    # The original tests, verbatim.
│   ├── NativeMethods/        # P/Invoke shim, verbatim.
│   ├── packages/             # Original NuGet caches (left for completeness).
│   ├── tools/                # Original Cake/build tooling.
│   ├── HuntAndPeck.sln
│   ├── SolutionInfo.cs
│   ├── build.cake
│   └── build.ps1
├── screenshots/              # Original marketing assets.
└── .gitignore-legacy         # The pre-existing .gitignore (renamed).
```

## The migration steps

Run these from the repo root, in order. Each is a separate commit so the
history reads cleanly.

### Step 1 — Move `src/` to `legacy/src/`

```powershell
mkdir legacy
git mv src legacy/src
```

> **Important:** use `git mv`, not a copy. We want history-preserving moves
> so `git log --follow` on legacy files still works.

### Step 2 — Move screenshots to `legacy/screenshots/`

```powershell
git mv screenshots legacy/screenshots
```

The new project will collect its own screenshots in `screenshots/` later.

### Step 3 — Archive the legacy `.gitignore`

```powershell
git mv .gitignore legacy/.gitignore-legacy
```

We will write a Rust-shaped `.gitignore` from scratch in M0.

### Step 4 — Add `legacy/README.md`

Single-file pointer (paste the following into `legacy/README.md`):

````markdown
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
````

### Step 5 — Top-level `.gitignore` (Rust-shaped)

After Step 3, the repo has no top-level `.gitignore`. Create one:

```gitignore
# Build artifacts
/target/
/dist/
**/*.rs.bk

# IDE
/.vs/
/.idea/
*.iml
*.suo
*.user

# OS
Thumbs.db
.DS_Store

# Logs
*.log

# Local dev
/.env
/.local/

# Profiling output
*.etl
*.wpa.json
flame.svg
```

The legacy `.gitignore-legacy` remains untouched for historical reference
inside `/legacy`.

### Step 6 — Update root `README.md`

Replace the existing legacy README with a forward-looking one. Suggested
content (paste into `README.md`):

````markdown
# Navigator

> Press a hotkey. Hints appear. Type letters. Act.
> A keyboard-native UI navigator for Windows. The successor to Hunt-and-Peck.

## Status

Pre-alpha. See [Agent/workflow/10-milestones.md](Agent/workflow/10-milestones.md)
for progress.

## Quick start (developers)

```powershell
git clone <this repo>
cd navigator
cargo build --workspace --release
.\target\release\nav-app.exe
```

Then press `Alt+;` over any focused window.

## Documentation

- **End users:** see `docs/` (post-v1).
- **Contributors:** start at `Agent/workflow/README.md`.
- **Legacy HAP:** see `legacy/`. Read-only.

## License

MIT OR Apache-2.0. The legacy HAP code retains its original license inside
`legacy/`.
````

### Step 7 — Verify

```powershell
git status
cargo --version
ls Agent/workflow
ls legacy
```

Then push the branch, open a PR titled "M0: legacy migration + workspace
skeleton", get CI green, merge.

## What if a contributor edits `/legacy`?

A pre-commit / pre-push hook (added in M0) refuses commits that modify
files under `legacy/`. The check is:

```bash
# tools/git-hooks/pre-commit
if git diff --cached --name-only | grep -q '^legacy/'; then
  echo "Refusing to modify /legacy. It is read-only reference."
  exit 1
fi
```

CI does the same check on PRs. Override with a PR description tag
`[legacy-edit-allowed: <reason>]` when reference material itself genuinely
needs an update (e.g. the legacy README points to a 404'd link).

## What we explicitly *take* from legacy

The following design decisions and constants are inherited intentionally:

| Carry-over                         | Source                                                   | Where it lives in new code         |
|------------------------------------|----------------------------------------------------------|------------------------------------|
| Default hotkey `Alt+;`             | `App.xaml.cs`, settings                                  | `assets/default-config.toml`       |
| Hint alphabet `S A D F J K L E W C M P G H` | `Services/HintLabelService.cs`                  | `assets/default-config.toml`       |
| Vimium-style label algorithm       | `Services/HintLabelService.cs::GetHintStrings`           | `nav-core/src/label.rs`            |
| Pattern dispatch order             | `Services/UiAutomationHintProviderService.cs::CreateHint`| `nav-uia/src/pattern.rs`           |
| Coordinate transform approach      | `Extensions/RectExtensions.cs`                           | `nav-uia/src/coords.rs`            |
| `/hint` and `/tray` CLI verbs      | `App.xaml.cs`                                            | `nav-config/src/cli.rs`            |

These ports must include a `// Legacy parity: see legacy/...` comment on
the *first* commit that introduces them, so future readers can find the
source.

## What we deliberately leave behind

| Dropped                              | Reason                                                |
|--------------------------------------|-------------------------------------------------------|
| WPF / XAML                           | Performance, see `05-performance-strategy.md`.        |
| Caliburn.Micro / MVVM frameworks     | Overhead for a 200ms overlay.                         |
| NuGet, Cake build, .csproj           | Cargo replaces these.                                 |
| `Form`-based hidden window for hooks | Replaced by a true message-only `HWND_MESSAGE` window.|
| Per-element COM property fetches     | Replaced by `BuildUpdatedCache`.                      |
| Single-monitor assumption            | Multi-monitor and per-monitor DPI from day one.       |
| Global mutable services & DI graph   | Pure functions + explicit ownership.                  |

## Verifying the migration

After Step 7, the following must be true:

- `cargo check --workspace` exits 0 in < 5 s (empty workspace).
- `git log --follow legacy/src/HuntAndPeck/Services/HintLabelService.cs`
  shows the original commit history (proves `git mv` worked).
- Top-level `README.md` no longer mentions Hunt-and-Peck except as a
  reference to `legacy/`.
- CI workflow ignores `legacy/` (no `*.csproj` builds attempted).
- The pre-commit hook blocks edits to `legacy/` in a manual test.

When all six are true, M0 is complete.
