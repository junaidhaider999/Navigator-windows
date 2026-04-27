//! M9 acceptance: smoke + repeatability on a stable system HWND (see `Agent/workflow/m9-acceptance.md`).

#![cfg(windows)]

use std::sync::Mutex;

use nav_uia::{
    EnumOptions, FallbackPolicy, M9_DEFAULT_BUDGET_HWND_MS, M9_DEFAULT_BUDGET_MSAA_MS,
    M9_DEFAULT_BUDGET_UIA_MS, UiaHwnd, UiaRuntime,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::w;

/// UIAutomation + COM cache construction is not reliable when multiple `UiaRuntime::new` run in parallel.
static UIA_M9_LOCK: Mutex<()> = Mutex::new(());

/// Taskbar; present on every interactive Windows session (incl. GitHub Actions `windows-latest`).
fn shell_tray_hwnd() -> UiaHwnd {
    let hwnd =
        unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.expect("FindWindowW(Shell_TrayWnd)");
    assert!(!hwnd.is_invalid(), "Shell_TrayWnd not found (no shell?)");
    hwnd
}

#[test]
fn m9_config_defaults_match_nav_config_seed() {
    // Keep aligned with `nav_config::default_budget_uia` / `config.toml` seeds.
    let o = EnumOptions::default();
    assert_eq!(o.budget_uia_ms, M9_DEFAULT_BUDGET_UIA_MS);
    assert_eq!(o.budget_msaa_ms, M9_DEFAULT_BUDGET_MSAA_MS);
    assert_eq!(o.budget_hwnd_ms, M9_DEFAULT_BUDGET_HWND_MS);
    assert_eq!(M9_DEFAULT_BUDGET_UIA_MS, 25);
    assert_eq!(M9_DEFAULT_BUDGET_MSAA_MS, 8);
    assert_eq!(M9_DEFAULT_BUDGET_HWND_MS, 5);
}

#[test]
fn m9_uia_runtime_com_init() {
    let _g = UIA_M9_LOCK.lock().expect("uia m9 lock");
    let _ = UiaRuntime::new().expect("COM + UIAutomation");
}

#[test]
fn m9_enumerate_taskbar_each_policy_succeeds() {
    let _g = UIA_M9_LOCK.lock().expect("uia m9 lock");
    let rt = UiaRuntime::new().expect("uia");
    let hwnd = shell_tray_hwnd();
    for policy in [
        FallbackPolicy::Auto,
        FallbackPolicy::UiaOnly,
        FallbackPolicy::MsaaOnly,
    ] {
        let mut opts = EnumOptions {
            max_elements: 64,
            fallback: policy,
            ..Default::default()
        };
        if matches!(policy, FallbackPolicy::MsaaOnly) {
            // MSAA can return many children on the tray; keep the test fast.
            opts.max_elements = 32;
        }
        let res = rt.enumerate(hwnd, &opts);
        assert!(
            res.is_ok(),
            "policy={policy:?} err={:?}",
            res.as_ref().err()
        );
    }
}

#[test]
fn m9_enumerate_auto_idempotent() {
    let _g = UIA_M9_LOCK.lock().expect("uia m9 lock");
    let rt = UiaRuntime::new().expect("uia");
    let hwnd = shell_tray_hwnd();
    let opts = EnumOptions {
        max_elements: 64,
        ..Default::default()
    };
    let a = rt
        .enumerate(hwnd, &opts)
        .expect("first enumerate")
        .hints
        .len();
    let b = rt
        .enumerate(hwnd, &opts)
        .expect("second enumerate")
        .hints
        .len();
    assert_eq!(a, b, "same HWND + opts should yield same count");
}

/// Repeat the full Auto ladder on a live window; catches COM / UIA resource leaks and panics.
#[test]
fn m9_reliability_triggers() {
    const N: usize = 80;
    let _g = UIA_M9_LOCK.lock().expect("uia m9 lock");
    let rt = UiaRuntime::new().expect("uia");
    let hwnd = shell_tray_hwnd();
    let opts = EnumOptions {
        max_elements: 32,
        ..Default::default()
    };
    for i in 0..N {
        let r = rt.enumerate(hwnd, &opts);
        assert!(r.is_ok(), "iter {i}: {:?}", r.err());
    }
}
