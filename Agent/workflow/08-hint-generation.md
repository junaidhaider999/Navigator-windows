# 08 — Hint Generation

> Hints exist to be typed fast. Everything in this document is in service of
> "the user's fingers reach the right keys without thinking."

## Goals, in priority order

1. **Short hints come first.** The most likely targets get one-character
   labels.
2. **Home row only by default.** `S A D F J K L` are reachable without finger
   travel.
3. **Prefix-free.** No label is a prefix of another. Typing the full label
   commits without a timeout.
4. **Layout-aware.** Targeting a button with a one-letter hint is great
   *until* that letter is the prefix of three other hints. The algorithm
   trades depth for breadth.

## Default alphabet

```
['s', 'a', 'd', 'f', 'j', 'k', 'l', 'e', 'w', 'c', 'm', 'p', 'g', 'h']
```

14 characters. Same as legacy HAP. Properties:

- All home-row first (`s a d f j k l`), then near-row.
- All ASCII, all unambiguous on a US-QWERTY keyboard.
- Ordering matters: short labels are drawn from the front of the array.

Reasons we **do not** use digits in the default:

- Digits require a finger trip away from the home row.
- Number-row keys are often pre-bound (browser tabs, etc.).

Configurable via `config.toml`. Layout-aware translation happens in
`nav-input::keymap` so AZERTY/QWERTZ users can keep the *physical* keys.

## Algorithm: capacity-aware vimium-style distribution

This algorithm is **adapted from vimium** (which solves the same problem for
links in a web page) and matches HAP's behavior — important so that long-time
HAP users feel no muscle memory loss.

Given:

- `N` = number of hints needed
- `A` = `alphabet.len()` (e.g. 14)
- `D` = number of digits required = `ceil(log_A(N))` if `N > 0`, else 0

We split labels into:

- **Short labels** of length `D - 1`
- **Long labels** of length `D`

The number of short labels is chosen to maximize their count *without*
breaking the prefix-free property:

```
whole       = A^D
short_count = (whole - N) / A          // integer floor
long_count  = N - short_count
long_prefix = whole / A - short_count
```

For `D >= 2`:

- The first `long_count` labels are emitted at length `D`, skipping the
  prefixes consumed by the short labels.
- The next `short_count` labels are emitted at length `D - 1`.

The labels are then **reversed** (vimium does this; it makes the visually
distinguishing character appear *first* when read top-to-bottom in a wide
hint scatter, which speeds typing).

### Worked example

`N = 25`, `A = 14`:

- `D = ceil(log_14(25)) = 2`
- `whole = 14^2 = 196`
- `short_count = (196 − 25) / 14 = 12`
- `long_count = 25 − 12 = 13`
- `long_prefix = 196 / 14 − 12 = 2`

So 13 two-letter labels are issued (with prefixes drawn from the alphabet
positions reserved for them), then 12 one-letter labels. Result: 12 of the
25 hints can be hit with a single keypress.

### Worked example, the boundary

`N = 14`: 14 one-letter labels. Always.

`N = 15`: `D = 2`, `short_count = 13`, `long_count = 2`. We get 13
single-letter hints and 2 two-letter hints. The transition from "all one
letter" to "almost all one letter" happens at exactly 15 — same as HAP.

`N = 196`: 196 two-letter hints, all unique.

`N = 197`: `D = 3`, three-letter labels appear, distributed by the same
formula.

## Layout-aware ranking

Generation produces a **set** of labels; ranking decides which hint gets
which label. We assign by descending priority:

```
priority(hint) = w_short * is_short_target(hint)
              + w_proximity * proximity_to_focus(hint)
              + w_kind * kind_weight(hint)
              + w_size * (1 / area(hint))
```

Tunables (defaults):

| Term                 | Default weight | Rationale                                              |
|----------------------|----------------|--------------------------------------------------------|
| `is_short_target`    | 0.0 (disabled) | Reserved for v2; lets users mark "primary" elements.   |
| `proximity_to_focus` | 1.0            | Hints near the currently focused control are likeliest.|
| `kind_weight`        | 0.6            | Buttons > toggles > selection > expand > editable.     |
| `1 / area`           | 0.2            | Small targets are usually denser & easier to mistarget.|

`proximity_to_focus` is computed as
`1 / (1 + manhattan(center(hint), center(focused_rect)))`, normalized.

The N highest-priority hints get the N short labels. Long labels go to the
rest in the same priority order.

This is what makes "the close-button-in-the-corner" not get `JK` when there's
also a `Save` button under your cursor.

## Filtering

When the user types a character `c`:

1. Translate `c` → `c'` via the layout map (so AZERTY users typing `q`
   matches `a` if they configured QWERTY alphabet).
2. Append to the current prefix `P := P + c'`.
3. For each visible hint `h`:
   - If `h.label.starts_with(P)` → keep.
   - Else → mark as filtered-out.
4. Count remaining matches:
   - `0` → cancel session (audible beep optional, off by default).
   - `1` → invoke that hint immediately, end session.
   - `> 1` → emit a render update with the new visibility mask.

The rule "single match commits immediately" is what makes Navigator feel
faster than a competitor that requires Enter to confirm. Hints are
**prefix-free**, so committing on `1 match` is *always* correct.

### What about typos?

`nav-core::filter` exposes a single character of "undo": `Backspace` removes
the last character from `P`, restoring whatever hints it filtered out. We do
**not** implement fuzzy match — that would break the prefix-free guarantee.

## Edge cases

### Zero hints

If enumeration returns 0 elements:

1. Emit a 250 ms tray-icon flash (optional, default off).
2. Do not show an empty overlay. The user gets immediate feedback that
   nothing was found.
3. Log the HWND, class name, and process name to the diagnose log.

### Massive hint count (>1000)

We hard-cap at `EnumOptions::max_elements` (default 1024). If exceeded:

1. We pick the 1024 highest-ranked.
2. We log a warning.
3. UX is unaffected, but ranking matters more.

### Overlapping hint rectangles

The planner detects overlap by sorting hints by `(y, x)` and running an
O(N) sweep. Overlapping pills are pushed to the next free quadrant
(TL → TR → BL → BR of the parent rect) by the renderer. The label assignment
is unchanged; only the *position* of the pill moves.

### Reserved characters

- `Esc` always cancels.
- `Backspace` always undoes one character.
- The hotkey itself, re-pressed, cancels (optional, default on).
- Any non-alphabet character cancels (optional, default off — the
  alternative, ignoring, is less surprising).

These behaviors are **outside** the alphabet and never appear as labels.

## Properties we test (proptest)

- `forall(N in 0..5000)`: `generate_labels(N).len() == N`.
- `forall(N in 0..5000)`: labels are pairwise prefix-free.
- `forall(N in 0..5000)`: every label is non-empty and uses only alphabet
  chars.
- `forall(N in 0..5000)`: short-label count is the maximum possible without
  breaking prefix-free.
- For `N <= A`, every label has length 1.
- For `A < N <= A^2`, label length is exactly 1 or 2.

## Unit tests we keep

Inherited from the legacy HAP test, ported to Rust:

```text
hint_count == 0  → []
hint_count == 1  → ["S"]
hint_count == 14 → all 14 single chars in alphabet order, reversed
hint_count == 15 → 13 singles + 2 doubles, prefix-free
hint_count == 196 → 196 doubles, all unique
hint_count == 1024 → tail uses triples; root prefixes cover all triples
```

If a future PR claims a "better" algorithm, it must ship these test cases
and prove latency improvements measurable in `nav-bench/label`.
