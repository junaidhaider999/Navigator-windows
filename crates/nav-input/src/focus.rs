//! Detect whether keyboard focus is in a control where typing plain `/` should reach the app.
//!
//! Heuristics: terminal surfaces (process + HWND classes), Win32 edit classes, then UIA focused leaf
//! and ancestors for Edit / Combo / Spinner. **`Document` is only honored on the focused element**
//! (real editors), not on WinUI ancestors that wrap entire windows — that blocked `/` in modern UI.
//! Integrated terminals inside IDEs (same process as the editor) may still need **`Alt+;`**.

use std::sync::Once;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, CUIAutomation8, IUIAutomation, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_SpinnerControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetParent, GetWindowThreadProcessId,
    GUITHREADINFO,
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
            if TERMINAL_EXE_BASENAMES.iter().any(|&e| e == base.as_str()) {
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
    if f.is_invalid() {
        None
    } else {
        Some(f)
    }
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
        || (lower.contains("textbox") && !lower.contains("calendar") && !lower.contains("navigation"))
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
    Some(std::string::String::from_utf16_lossy(&buf[..len]).into())
}

/// True when UIA indicates real text entry. Ancestor **`Document`** is ignored — WinUI often wraps
/// whole windows in `Document`, which falsely suppressed plain `/` outside editors.
fn uia_focus_implies_text_input(automation: &IUIAutomation) -> bool {
    let Ok(focused) = (unsafe { automation.GetFocusedElement() }) else {
        return false;
    };
    let browser_doc_pass_through = foreground_process_browserish();

    if let Ok(ct) = unsafe { focused.CurrentControlType() } {
        if ct == UIA_EditControlTypeId
            || ct == UIA_ComboBoxControlTypeId
            || ct == UIA_SpinnerControlTypeId
        {
            return true;
        }
        // True document editors (Word canvas, etc.). Browsers use `Document` for pages — skip via browser list.
        if ct == UIA_DocumentControlTypeId && !browser_doc_pass_through {
            return true;
        }
    }

    let Ok(walker) = (unsafe { automation.ControlViewWalker() }) else {
        return false;
    };

    let mut cur = unsafe { walker.GetParentElement(&focused) }.ok();
    for _ in 0..27 {
        let Some(e) = cur.take() else {
            break;
        };
        if let Ok(ct) = unsafe { e.CurrentControlType() } {
            if ct == UIA_EditControlTypeId
                || ct == UIA_ComboBoxControlTypeId
                || ct == UIA_SpinnerControlTypeId
            {
                return true;
            }
            // Do **not** treat `Document` here — it is often a WinUI scroll/root, not the focused editor.
        }
        cur = unsafe { walker.GetParentElement(&e) }.ok();
    }
    false
}
