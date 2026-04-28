//! Window/process probe for enumeration ladder selection (Win32-first vs UIA-first vs Chromium tuning).

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
};

use crate::hwnd::UiaHwnd;
use crate::options::EnumerationStrategyMode;

/// Which backend ordering [`crate::runtime::UiaRuntime::enumerate`] uses for [`crate::options::FallbackPolicy::Auto`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedLadder {
    /// UIA → MSAA → HWND (legacy Navigator default).
    UiaFirst,
    /// HWND → MSAA → UIA (Explorer / classic Win32).
    Win32First,
}

/// Process/class probe + suggested ladder before `[hints].enumeration_strategy` override.
#[derive(Clone, Debug)]
pub struct WindowProbe {
    pub pid: u32,
    pub class_name: String,
    pub exe_basename: String,
    pub suggested_ladder: ResolvedLadder,
    /// Prefer single-threaded UIA materialization (Chromium / Electron heavy trees).
    pub suggested_disable_parallel: bool,
}

fn basename_from_wide_path(path_wide_hint: &[u16]) -> String {
    let os = OsString::from_wide(path_wide_hint.split(|&c| c == 0).next().unwrap_or(&[]));
    Path::new(&os)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn read_process_path(pid: u32) -> Option<String> {
    let Ok(h) = (unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
    }) else {
        return None;
    };
    let mut buf = vec![0u16; 4096];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(h) };
    if ok.is_err() || len == 0 {
        return None;
    }
    let slice = &buf[..len as usize];
    Some(basename_from_wide_path(slice))
}

fn read_class_name(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n == 0 {
        return String::new();
    }
    let slice = &buf[..n as usize];
    OsString::from_wide(slice)
        .to_string_lossy()
        .into_owned()
}

fn is_chromium_family(class: &str, exe_base: &str) -> bool {
    let c = class.to_ascii_lowercase();
    if c.contains("chrome_widgetwin") || c.contains("chromium") || c.contains("webview") {
        return true;
    }
    let e = exe_base.to_ascii_lowercase();
    matches!(
        e.as_str(),
        "chrome.exe"
            | "msedge.exe"
            | "brave.exe"
            | "vivaldi.exe"
            | "opera.exe"
            | "discord.exe"
            | "slack.exe"
            | "teams.exe"
            | "code.exe"
            | "cursor.exe"
            | "devenv.exe"
            | "electron.exe"
    )
}

fn is_win32_hwnd_first_candidate(class: &str, exe_base: &str) -> bool {
    if exe_base.eq_ignore_ascii_case("explorer.exe") {
        return true;
    }
    matches!(
        class,
        "CabinetWClass" | "ExploreWClass" | "Progman" | "WorkerW"
    )
}

/// Inspect HWND → PID, window class, exe basename, and suggested ladder.
#[must_use]
pub fn probe_window(hwnd: UiaHwnd) -> WindowProbe {
    let hwnd = HWND(hwnd.0);
    let mut pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let class_name = read_class_name(hwnd);
    let exe_basename = read_process_path(pid).unwrap_or_else(|| "unknown".to_string());

    let mut suggested_ladder = ResolvedLadder::UiaFirst;
    let mut suggested_disable_parallel = false;

    if is_chromium_family(&class_name, &exe_basename) {
        suggested_ladder = ResolvedLadder::UiaFirst;
        suggested_disable_parallel = true;
    } else if is_win32_hwnd_first_candidate(&class_name, &exe_basename) {
        suggested_ladder = ResolvedLadder::Win32First;
    }

    WindowProbe {
        pid,
        class_name,
        exe_basename,
        suggested_ladder,
        suggested_disable_parallel,
    }
}

/// Merge `[hints].enumeration_strategy` with probe defaults.
#[must_use]
pub fn resolve_enumeration_behavior(
    mode: EnumerationStrategyMode,
    probe: &WindowProbe,
) -> (ResolvedLadder, bool /* disable_uia_parallel */) {
    match mode {
        EnumerationStrategyMode::Auto => (
            probe.suggested_ladder,
            probe.suggested_disable_parallel,
        ),
        EnumerationStrategyMode::UiaFirst => (ResolvedLadder::UiaFirst, false),
        EnumerationStrategyMode::Win32First => (ResolvedLadder::Win32First, false),
        EnumerationStrategyMode::ChromiumFast => (ResolvedLadder::UiaFirst, true),
    }
}

/// Window title hash + outer rect (invalidates hot cache on move/rename).
#[must_use]
pub fn window_cache_key(hwnd: UiaHwnd) -> (u64, i32, i32, i32, i32) {
    let h = HWND(hwnd.0);
    let mut buf = vec![0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    let slice_len = n.min(buf.len());
    let title_fp = fnv1a_utf16(&buf[..slice_len]);
    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(h, &mut r) };
    (title_fp, r.left, r.top, r.right, r.bottom)
}

fn fnv1a_utf16(s: &[u16]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &u in s {
        for b in u.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(1099511628211);
        }
    }
    h
}
