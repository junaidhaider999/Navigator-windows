# 06 — Windows APIs

> The minimum set of Windows APIs we use, why we picked each one, and the
> precise fallback ladder for unreachable elements.

## Crate of choice: `windows` (Microsoft official)

We use the official Microsoft `windows` crate (not `winapi`).

Reasons:

- Auto-generated from the official Windows metadata. Up to date with WinSDK.
- Compiles cleanly with our pinned MSRV.
- COM types come with `IUnknown`-derived smart pointers and proper
  `QueryInterface` ergonomics.
- Tree-shakes by feature. We only enable the subsystems we touch:

```toml
[dependencies.windows]
version = "0.59"
features = [
    "Win32_Foundation",
    "Win32_System_Com",
    "Win32_System_Threading",
    "Win32_System_LibraryLoader",
    "Win32_UI_Accessibility",        # UIA + MSAA (IAccessible).
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_HiDpi",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Direct2D",
    "Win32_Graphics_Direct2D_Common",
    "Win32_Graphics_DirectComposition",
    "Win32_Graphics_DirectWrite",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_System_DataExchange",     # For tray notify (only if we add the tray).
]
```

We do not add features speculatively. Each feature added must point to a
specific call site.

## API choices, by subsystem

### Hotkey

| Need                          | API                                     | Why                                  |
|-------------------------------|-----------------------------------------|--------------------------------------|
| Global chord trigger          | `RegisterHotKey` / `WM_HOTKEY`          | Lowest latency. Kernel-mediated.     |
| Avoid storming on key-hold    | `MOD_NOREPEAT`                          | Mandatory.                           |
| Receive `WM_HOTKEY`           | Message-only window (`HWND_MESSAGE`)    | No taskbar entry, no z-order cost.   |

Anti-choice: a low-level keyboard hook **as the primary trigger**. LL hooks
have an OS timeout (`LowLevelHooksTimeout`, default 300 ms) and run on every
keystroke globally — paying that cost when no hint mode is active is
unacceptable. We use the LL hook **only** while in hint mode (see below).

### Foreground window

| Need                          | API                                     | Why                                  |
|-------------------------------|-----------------------------------------|--------------------------------------|
| HWND of focused app           | `GetForegroundWindow`                   | One syscall, no marshalling.         |
| Window rect (screen)          | `GetWindowRect`                         | Fastest source of bounds.            |
| Window owner process / thread | `GetWindowThreadProcessId`              | Used for diagnostic logging only.    |
| DPI for the window's monitor  | `GetDpiForWindow`                       | Per-monitor DPI accurate (Win10+).   |

Snapshot the foreground HWND at hotkey time, *before* the overlay shows.
Otherwise we steal focus and enumerate ourselves.

### UI Automation (primary enumeration path)

| Need                                     | API                                                |
|------------------------------------------|----------------------------------------------------|
| Automation singleton                     | `CoCreateInstance(CUIAutomation8)`                 |
| Element from HWND                        | `IUIAutomation::ElementFromHandle`                 |
| Cache request                            | `IUIAutomation::CreateCacheRequest`                |
| Cached subtree                           | `IUIAutomationElement::BuildUpdatedCache`          |
| Walk cached tree                         | `IUIAutomationTreeWalker` (`ControlViewWalker`)    |
| Per-element cached property              | `Cached*Property` family on the element            |
| Pattern access (cached)                  | `IUIAutomationElement::GetCachedPattern`           |

We prefer `CUIAutomation8` over `CUIAutomation` because v8 supports the
`AutomationElementMode_None` mode and additional pattern interfaces we may
later use (e.g. `IUIAutomationTextRange2`).

#### Patterns we dispatch to (in priority order)

1. `IUIAutomationInvokePattern` — buttons, hyperlinks. Most actions.
2. `IUIAutomationTogglePattern` — checkboxes, toggle buttons.
3. `IUIAutomationSelectionItemPattern` — list items, tabs.
4. `IUIAutomationExpandCollapsePattern` — tree nodes, dropdowns.
5. `IUIAutomationValuePattern` — editable fields where we want focus, not
   click. We call `SetValue` only when explicitly configured; default is to
   set focus and place the caret.
6. *Last resort* — `SendInput` mouse click at element center.

### MSAA (first fallback)

When UIA returns 0 elements (rare; happens on some legacy Win32 dialogs and
older Office surfaces):

| Need                                | API                                              |
|-------------------------------------|--------------------------------------------------|
| Acquire root                        | `AccessibleObjectFromWindow(hwnd, OBJID_CLIENT)` |
| Walk children                       | `IAccessible::accChild` + `accChildCount`        |
| Bounding rect                       | `IAccessible::accLocation`                       |
| Action                              | `IAccessible::accDoDefaultAction`                |

MSAA is older and faster on legacy Win32, but its tree is less
well-structured. We treat MSAA hits as `GenericClickable` unless they
expose a default action.

### Raw HWND (last resort)

When both UIA and MSAA return zero:

| Need                                | API                                              |
|-------------------------------------|--------------------------------------------------|
| Children                            | `EnumChildWindows`                               |
| Visibility                          | `IsWindowVisible`                                |
| Enabled                             | `IsWindowEnabled`                                |
| Class name                          | `RealGetWindowClassW`                            |
| Bounds                              | `GetWindowRect`                                  |
| Click                               | `SendInput` MOUSEEVENTF_LEFTDOWN/UP at center    |

Raw HWND clicks are the **least preferred** path. We move the mouse cursor
back to its prior position immediately after the click to avoid disturbing
the user's pointer. We do not use `PostMessage(WM_LBUTTONDOWN, ...)` — many
modern controls do not respond to synthetic messages, only to real input.

### Rendering

| Need                                | API                                              |
|-------------------------------------|--------------------------------------------------|
| GPU device                          | `D3D11CreateDevice` (BGRA, no debug)             |
| Composition device                  | `DCompositionCreateDevice2`                      |
| Swap chain for visual               | `IDXGIFactory2::CreateSwapChainForComposition`   |
| 2D context                          | `D2D1CreateFactory` → `ID2D1Device::CreateDeviceContext` |
| Text format                         | `DWriteCreateFactory` → `CreateTextFormat`       |
| Layered window                      | `WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP`      |
| DPI awareness                       | `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` (declared in manifest, not at runtime) |

DPI awareness is set in the **manifest**, not via API. Setting at runtime
is a footgun on Win10 because Windows may have already created HWNDs in the
"old" awareness mode. Manifest is authoritative.

### Input swallowing in hint mode

| Need                                | API                                              |
|-------------------------------------|--------------------------------------------------|
| Capture all keys globally           | `SetWindowsHookExW(WH_KEYBOARD_LL, ...)`         |
| Translate VK to char (layout-aware) | `ToUnicodeEx` with current `HKL`                 |
| Detect modifier state               | `GetAsyncKeyState`                               |
| Cancel the hook                     | `UnhookWindowsHookEx`                            |

The LL hook is installed **immediately before** showing the overlay and
uninstalled the moment the session ends. While installed, it consumes the
keystroke and posts a translated char to the input thread's channel. The
underlying app sees no keys.

### Single-instance and tray (later)

| Need                                | API                                              |
|-------------------------------------|--------------------------------------------------|
| Named-mutex lock                    | `CreateMutexW("Local\\Navigator-...")`           |
| Tray icon                           | `Shell_NotifyIconW`                              |
| Tray context menu                   | `TrackPopupMenu` on `WM_RBUTTONUP`               |
| Toast / balloon (sparingly)         | `NIF_INFO` flag on `Shell_NotifyIconW`           |

## Fallback ladder (canonical)

When the orchestrator asks "what hints can I show on this HWND?", the
order is:

```
┌──────────────────────────────────────────────────────────────────────┐
│ Step 1: UIA cached enumeration (the 95% path)                        │
│   • CUIAutomation8 + cached request + BuildUpdatedCache              │
│   • If element_count >= 1 → done.                                    │
└──────────────────────┬───────────────────────────────────────────────┘
                       │ element_count == 0
                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Step 2: MSAA enumeration                                             │
│   • AccessibleObjectFromWindow + accChild walk                       │
│   • Treat each child with non-empty rect and default action as a hint│
│   • If element_count >= 1 → done.                                    │
└──────────────────────┬───────────────────────────────────────────────┘
                       │ element_count == 0
                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Step 3: Raw HWND enumeration                                         │
│   • EnumChildWindows + IsWindowVisible + IsWindowEnabled             │
│   • Treat each as GenericClickable with center-click via SendInput   │
│   • If element_count >= 1 → done.                                    │
│   • Otherwise → emit "no hints" and exit cleanly.                    │
└──────────────────────────────────────────────────────────────────────┘
```

Each step has a hard time budget enforced by the orchestrator:

- Step 1: 25 ms. Must always complete.
- Step 2: 8 ms. Cancelled if it exceeds.
- Step 3: 5 ms. Cancelled if it exceeds.

If all three time out (extremely rare), the session is cancelled silently
and a tray balloon offers a "diagnose" link that captures a UIA dump.

## Window-class allow/deny lists

Some windows are best **not** hinted by default:

- `Shell_TrayWnd` — the taskbar. Use a dedicated taskbar hotkey (M2 stretch).
- `MultitaskingViewFrame` — Win+Tab task view. Already keyboard-driven.
- `Windows.UI.Core.CoreWindow` (modern shell surfaces) — unreliable.

`config.toml` exposes class-name and process-name exclusions. The defaults
exclude the items above. Users can customize.

## What we explicitly do not use

| API / approach                        | Why we refuse                                                       |
|---------------------------------------|---------------------------------------------------------------------|
| `SetWinEventHook` for tree changes    | Floods us with events 99% irrelevant. We rebuild the cache per sess. |
| `WH_GETMESSAGE` global hook           | Whole-system overhead during idle for negligible benefit.           |
| `IRawElementProviderSimple` providers | Implementing a UIA *provider* is for being-the-target, not the client. |
| WPF / WinUI / XAML                    | Wrong tradeoff for a 200ms overlay. See §05.                        |
| .NET host                             | Cold-start budget is 50ms; CLR startup alone exceeds it.            |
| Electron / web tech                   | Multi-second startup, hundreds of MB. No.                           |
| GDI / GDI+                            | No hardware accel, flicker, redirection issues with layered windows.|

## Permissions

- Navigator runs as the user. **No elevation.**
- We do **not** support hinting elevated windows from a non-elevated
  client. UIPI prevents us from injecting input into a higher-integrity
  process. Document this. Provide an "Run elevated" tray menu item that
  re-launches Navigator with `Verb=runas` for users who want to hint
  elevated apps.
- The LL keyboard hook does not require elevation, but is sometimes
  flagged by anti-cheat / EDR products. Document this in the README.
