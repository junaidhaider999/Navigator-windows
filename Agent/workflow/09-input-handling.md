# 09 — Input Handling

> The keyboard is Navigator's only interface. This document defines the
> input loop, the hotkey path, the hint-mode capture, and the precise rules
> for what we swallow and what we let through.

## Two distinct input regimes

Navigator's input lives in **two modes**, with a hard transition between
them:

```
   Idle ────hotkey────▶ HintMode ────done────▶ Idle
                            │
                            └──Esc / focus loss / 2nd hotkey──▶ Idle
```

| Mode      | Active mechanism                  | Cost when idle |
|-----------|-----------------------------------|----------------|
| Idle      | `RegisterHotKey` + `WM_HOTKEY`    | ~0 (kernel)    |
| HintMode  | LL keyboard hook + RegisterHotKey | hook callback / keystroke |

**Repo state:** `nav-input` installs **`WH_KEYBOARD_LL`** while `hint_mode` is
true (`crates/nav-input/src/thread.rs`), forwarding **`SessionKey`** events on
the same channel as **`WM_HOTKEY`**. `Alt+;` and other **Alt chords** are passed
through so the global hotkey still fires.

We never run an LL hook in Idle. Doing so would tax every keystroke globally,
not for our benefit.

## Idle mode: the hotkey path

```
                ┌────────────────────────────────────┐
                │       Input thread (boot)          │
                │  • CreateWindowEx(HWND_MESSAGE)    │
                │  • RegisterHotKey(...)             │
                │  • PeekMessageW loop on wnd        │
                └────────────────────────────────────┘
                                │
   user presses Alt+;           │  WM_HOTKEY arrives
                                ▼
                ┌────────────────────────────────────┐
                │  WM_HOTKEY handler                  │
                │  • LARGE_INTEGER perf-counter T0   │
                │  • foreground = GetForegroundWindow│
                │  • tx_event.send(Hotkey { T0, hwnd})│
                │  • return                          │
                └────────────────────────────────────┘
                                │
                                ▼
                       Orchestrator picks it up,
                       starts a session.
```

Total work in the hotkey handler: capture `QueryPerformanceCounter`, capture
HWND, push to a channel. **Nothing else.** Anything beyond this is
sub-optimal because the input thread message pump is shared with monitor
change events, tray clicks, and the LL hook in HintMode.

### Hotkey registration details

```rust
let mods  = MOD_ALT | MOD_NOREPEAT;        // 0x0001 | 0x4000
let vk    = VK_OEM_1 as u32;               // semicolon, US layout
let id    = 1;
RegisterHotKey(message_only_hwnd, id, mods, vk)?;
```

- `MOD_NOREPEAT` is mandatory. Without it, holding the chord generates a
  steady stream of `WM_HOTKEY` events.
- We register the hotkey on the input thread's message-only HWND, not on
  the overlay window. The overlay is `WS_EX_NOACTIVATE` and not a good
  hotkey target.
- A second hotkey (e.g. taskbar `Ctrl+;`) gets a distinct `id` so the
  handler can route correctly.

### Failure: hotkey already taken

If `RegisterHotKey` returns `0`, another app owns the chord. We:

1. Log the failure with the requested mod/vk pair.
2. Surface a tray balloon: "Navigator could not register Alt+;".
3. Do **not** silently pick a different hotkey. Surprise hotkeys are
   anti-keyboard-native.

## HintMode: the LL keyboard hook

The moment we transition into HintMode (after the orchestrator decides it
has hints and is ready to render):

```
nav_input::enter_hint_mode():
    SetWindowsHookExW(WH_KEYBOARD_LL, callback, hmod, 0)
    self.hint_mode = true
```

Inside the hook:

```rust
unsafe extern "system" fn ll_kbd(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code != HC_ACTION { return CallNextHookEx(None, code, w, l); }

    let kb = &*(l.0 as *const KBDLLHOOKSTRUCT);
    let is_down = w.0 == WM_KEYDOWN as usize || w.0 == WM_SYSKEYDOWN as usize;

    if !is_down { return CallNextHookEx(None, code, w, l); } // let key-up pass

    // Translate VK to char respecting the user's layout.
    if let Some(c) = translate(kb.vkCode, kb.scanCode) {
        // Forward to the orchestrator. Non-blocking try_send.
        let _ = TX_INPUT.try_send(InputEvent::HintKey(c));
        return LRESULT(1); // swallow
    }

    match VK(kb.vkCode as u16) {
        VK_ESCAPE => {
            let _ = TX_INPUT.try_send(InputEvent::HintCancel);
            return LRESULT(1);
        }
        VK_BACK => {
            let _ = TX_INPUT.try_send(InputEvent::HintBackspace);
            return LRESULT(1);
        }
        _ => CallNextHookEx(None, code, w, l) // unknown key passes through
    }
}
```

**Hard rules for the hook callback:**

1. **No allocation, no I/O, no logging.** Hook callbacks run on the global
   keystroke path. Anything beyond a fixed-time computation taxes every
   keystroke in the system.
2. **Channel send must be non-blocking.** A `try_send` failure is acceptable
   in pathological backpressure cases (orchestrator deadlocked, etc.); we
   prefer dropping the event to blocking the keyboard.
3. **Watchdog the timeout.** Windows revokes hooks that exceed
   `LowLevelHooksTimeout` (default 300 ms). If our callback ever takes >5 ms,
   that's a bug.

When HintMode ends:

```
nav_input::exit_hint_mode():
    UnhookWindowsHookEx(self.hook)
    self.hook = None
    self.hint_mode = false
```

We tear down the hook on every session end, even if the session was just
1 ms long. Leaving it installed is a global perf tax we cannot justify.

## Layout-aware char translation

The user has configured an alphabet of, e.g., `s a d f j k l ...`. They
might be on AZERTY where the `q` key produces `a`. We must respect physical
keys, not the produced character, *if* the user opted into "physical
layout" mode.

Two modes:

- **Logical (default):** the alphabet is a list of characters. Whatever the
  layout produces, we match against the `char`. So on AZERTY, the user
  pressing `q` matches the `'a'` in the alphabet — same physical position
  as on QWERTY.
- **Strict char mode:** match the produced `char` exactly. Useful for
  multilingual users with custom alphabets.

Implementation uses `ToUnicodeEx`:

```rust
let layout = GetKeyboardLayout(thread_id);   // current foreground thread
let mut buf = [0u16; 8];
let n = ToUnicodeEx(vk, scan, &kbd_state, &mut buf, 0, layout);
if n >= 1 {
    let utf16 = &buf[..n as usize];
    // Take first char, downcase ASCII, emit.
}
```

### Dead keys

`ToUnicodeEx` may return a negative count for dead keys (waiting for the
next press). In hint mode we treat dead keys as "no character", let the
real key arrive on the next press, and translate then.

## Modifier handling in hint mode

While hint mode is active:

- **Shift** + alphabet char — same as the unshifted char (we are
  case-insensitive).
- **Ctrl/Alt/Win** combos — passed through. This lets a user use
  `Ctrl+C` while a hint mode is somehow still active (it should not be,
  since key-down/up alone cancel).

We **swallow** plain alphabet keys; we do **not** swallow modifier-only
combos. This means the user can always escape a stuck hint mode with
`Ctrl+Shift+Esc` (Task Manager) or the Win key.

## State machine reference (orchestrator-side)

```
                          ┌──────────────────────┐
                          │        Idle          │
                          └──────────┬───────────┘
                                     │ Hotkey
                                     ▼
              ┌──────────────────────────────────┐
              │           Enumerating            │
              │   • snapshot HWND                │
              │   • dispatch UIA work            │
              └──────────┬───────────────────────┘
                         │ hints arrive
                         ▼
        no hints  ┌──────────────────────────────┐
       ◀──────────│            Visible            │
                  │   • LL hook installed        │
                  │   • render.show()            │
                  └──────────┬───────────────────┘
                             │ HintKey
                             ▼
                  ┌──────────────────────────────┐
                  │           Filtered           │
                  │   • render.update()          │
                  │   • single match → Invoking  │
                  │   • zero matches → cancel    │
                  └──────────┬───────────────────┘
                             │ single
                             ▼
                  ┌──────────────────────────────┐
                  │           Invoking           │
                  │   • LL hook removed          │
                  │   • render.hide()            │
                  │   • dispatch invoke()        │
                  └──────────┬───────────────────┘
                             ▼
                          Idle
```

Cancel transitions (any → Idle):

- **Esc** at any time after the hook is installed.
- **Foreground change** detected by the orchestrator on a 50 ms heartbeat
  (`GetForegroundWindow != session.captured_hwnd`).
- **Re-press of the hotkey** during HintMode (configurable; default cancels).
- **Timeout** (configurable; default 30 s, off by default).

## Cleanup invariants

If the orchestrator panics or dies for any reason during HintMode:

1. The render thread receives a `Hide` command on its channel close, hides.
2. The input thread's `Drop` impl removes the hook unconditionally.

We test this by deliberately panicking the orchestrator in a debug build and
asserting the LL hook count goes to zero (`UnhookWindowsHookEx` returns
`true`).

## Special hotkeys (M2-M3)

| Action          | Default     | Notes                                         |
|-----------------|-------------|-----------------------------------------------|
| Hint mode       | `Alt+;`     | Primary trigger.                              |
| Taskbar hint    | `Ctrl+;`    | Hints over taskbar items only.                |
| Debug enum      | `Alt+Shift+;` | Dev-only: dumps UIA tree to log.            |

Each is a separate `RegisterHotKey` registration with its own id. The
handler routes by id.

## Testing strategy

- **Unit:** `keymap` tests for VK translation across QWERTY, AZERTY, QWERTZ,
  Dvorak.
- **Integration:** a fixture binary spawns Navigator, fires `keybd_event`
  for `Alt+;`, watches for hotkey latency log line. Repeat 100x, assert
  P95 < 1 ms hot-key dispatch.
- **Manual:** AV/EDR products sometimes flag LL hooks. Maintain a
  spreadsheet of "tested with X, works/breaks". Surface known issues in
  the README.
