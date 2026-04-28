//! Detect whether keyboard focus is in a control where typing plain `/` should reach the app.
//!
//! Heuristics: terminals first; Win32 class of **`hwndFocus`** (`GetGUIThreadInfo`); then UIA only when
//! **`GetFocusedElement`** aligns with **`ElementFromHandle(hwndFocus)`** (same node or descendant).
//! If keyboard-focus HWND and UIA “focused” element disagree (caret/UI oddities), we **do not** suppress
//! plain `/` — keystrokes would not go to that field anyway, so Navigator activation is safe.

use std::sync::Once;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationElement, UIA_ComboBoxControlTypeId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_SpinnerControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetParent,
    GetWindowThreadProcessId,
};
use windows::core::PWSTR;

static COM_INIT: Once = Once::new();

/// Known terminal hosts / shells — plain `/` must reach the PTY (Navigator stays closed).
const TERMINAL_EXE_BASENAMES: &[&str] = &[
    "windowsterminal.exe",
    "wt.exe",
    "openconsole.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "bash.exe",
    "mintty.exe",
    "comemu.exe",
    "comemu64.exe",
    "wezterm-gui.exe",
    "wezterm.exe",
    "alacritty.exe",
    "kitty.exe",
    "tabby.exe",
    "fluentterminal.exe",
    "hyper.exe",
    "electerm.exe",
    "mobaxterm.exe",
    "rio.exe",
    "ghostty.exe",
    "conhost.exe",
    "ssh.exe",
];

pub(crate) fn ensure_com_apartment() {
    COM_INIT.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    });
}

/// Returns `true` when plain `/` must be passed through to the focused app (Navigator stays closed).
pub(crate) fn focused_control_suppresses_plain_slash_hotkey() -> bool {
    ensure_com_apartment();
    if suppress_for_terminal_context() {
        return true;
    }
    if focus_hwnd_implies_text_input() {
        return true;
    }
    let Ok(automation) = (unsafe {
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    })
    .or_else(|_| unsafe {
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }) else {
        return false;
    };
    uia_focus_implies_text_input(&automation)
}

/// Terminals: foreground process name and/or HWND classes for console surfaces.
fn suppress_for_terminal_context() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != 0 {
        if let Some(path) = process_exe_path(pid) {
            let base = exe_basename_lower(&path);
            if TERMINAL_EXE_BASENAMES.contains(&base.as_str()) {
                return true;
            }
        }
    }
    if hwnd_is_terminal_surface(hwnd) {
        return true;
    }
    let Some(focus) = foreground_thread_focus_hwnd() else {
        return false;
    };
    if hwnd_is_terminal_surface(focus) {
        return true;
    }
    let mut h = focus;
    for _ in 0..12 {
        let Ok(parent) = (unsafe { GetParent(h) }) else {
            break;
        };
        if parent.is_invalid() {
            break;
        }
        if hwnd_is_terminal_surface(parent) {
            return true;
        }
        h = parent;
    }
    false
}

fn exe_basename_lower(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn hwnd_is_terminal_surface(hwnd: HWND) -> bool {
    let mut buf = [0u16; 160];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n == 0 {
        return false;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    let lower = s.to_ascii_lowercase();
    lower.contains("cascadia")
        || lower == "consolewindowclass"
        || lower.contains("pseudoconsole")
        || lower == "mintty"
        || lower.starts_with("vt_win32")
        || lower.contains("vttextbox") // ConPTY / some hosts
}

fn foreground_thread_focus_hwnd() -> Option<HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if tid == 0 {
        return None;
    }
    let mut gti = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(tid, &mut gti) }.ok()?;
    let f = gti.hwndFocus;
    if f.is_invalid() { None } else { Some(f) }
}

fn focus_hwnd_implies_text_input() -> bool {
    foreground_thread_focus_hwnd().is_some_and(hwnd_class_implies_text_editing)
}

fn hwnd_class_implies_text_editing(hwnd: HWND) -> bool {
    let mut buf = [0u16; 160];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n == 0 {
        return false;
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    let lower = s.to_ascii_lowercase();
    matches!(lower.as_str(), "edit" | "scintilla")
        || lower.starts_with("richedit")
        || (lower.contains("richtext") && !lower.contains("navigation"))
        || (lower.contains("textbox")
            && !lower.contains("calendar")
            && !lower.contains("navigation"))
}

fn foreground_process_browserish() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return false;
    }
    let Some(path) = process_exe_path(pid) else {
        return false;
    };
    let base = exe_basename_lower(&path);
    matches!(
        base.as_str(),
        "chrome.exe"
            | "msedge.exe"
            | "firefox.exe"
            | "brave.exe"
            | "vivaldi.exe"
            | "opera.exe"
            | "arc.exe"
            | "zen.exe"
    ) || base.starts_with("chrome")
        || base.contains("msedgewebview")
        || base == "webviewhost.exe"
}

fn process_exe_path(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut buf = vec![0u16; 520];
    let mut size = buf.len() as u32;
    let r = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    r.ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(std::string::String::from_utf16_lossy(&buf[..len]))
}

/// True when UIA's focused element matches **keyboard** focus (`hwndFocus`), and that element is a
/// text surface. No unconditional ancestor Edit walk — misaligned focus/caret cases keep `/` usable.
fn uia_focus_implies_text_input(automation: &IUIAutomation) -> bool {
    let Some(hwnd_focus) = foreground_thread_focus_hwnd() else {
        return false;
    };
    let Ok(el_hwnd) = (unsafe { automation.ElementFromHandle(hwnd_focus) }) else {
        return false;
    };
    let Ok(fe) = (unsafe { automation.GetFocusedElement() }) else {
        return false;
    };

    if !uia_focus_aligned_with_keyboard_hwnd(automation, &fe, &el_hwnd) {
        return false;
    }

    let browser_doc_pass_through = foreground_process_browserish();

    if let Ok(ct) = unsafe { fe.CurrentControlType() } {
        if ct == UIA_EditControlTypeId
            || ct == UIA_ComboBoxControlTypeId
            || ct == UIA_SpinnerControlTypeId
        {
            return true;
        }
        if ct == UIA_DocumentControlTypeId && !browser_doc_pass_through {
            return true;
        }
    }
    false
}

/// `GetFocusedElement` must be the same node as `ElementFromHandle(hwndFocus)`, or a **descendant**
/// of it — i.e. real input goes to the HWND that owns keyboard focus.
fn uia_focus_aligned_with_keyboard_hwnd(
    automation: &IUIAutomation,
    fe: &IUIAutomationElement,
    el_hwnd: &IUIAutomationElement,
) -> bool {
    if unsafe { automation.CompareElements(fe, el_hwnd) }
        .map(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    let Ok(walker) = (unsafe { automation.ControlViewWalker() }) else {
        return false;
    };
    let mut cur = unsafe { walker.GetParentElement(fe) }.ok();
    for _ in 0..64 {
        let Some(e) = cur.take() else {
            return false;
        };
        if unsafe { automation.CompareElements(&e, el_hwnd) }
            .map(|v| v.as_bool())
            .unwrap_or(false)
        {
            return true;
        }
        cur = unsafe { walker.GetParentElement(&e) }.ok();
    }
    false
}
