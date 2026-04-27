//! Notification area icon + context menu (M10).
#![allow(unsafe_op_in_unsafe_fn)]

use std::mem::zeroed;
use std::sync::OnceLock;

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFY_ICON_DATA_FLAGS, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, HWND_MESSAGE, LoadIconW, MF_STRING, RegisterClassExW,
    SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_RBUTTONUP, WNDCLASSEXW,
};
use windows::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, MSG};

use windows::core::w;

/// Events delivered to the main thread from the tray menu.
#[derive(Debug, Clone, Copy)]
pub enum TrayEvent {
    Reload,
    OpenConfigFolder,
    Diagnose,
    About,
    Quit,
}

static TRAY_TX: OnceLock<Sender<TrayEvent>> = OnceLock::new();

const WM_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 88;
const IDM_RELOAD: usize = 2001;
const IDM_OPEN: usize = 2002;
const IDM_DIAG: usize = 2003;
const IDM_ABOUT: usize = 2004;
const IDM_EXIT: usize = 2005;

fn wide_tip(s: &str) -> [u16; 128] {
    let mut buf = [0u16; 128];
    for (i, u) in s.encode_utf16().take(127).enumerate() {
        buf[i] = u;
    }
    buf
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_TRAY && lparam.0 as u32 == WM_RBUTTONUP {
        if let Ok(hmenu) = CreatePopupMenu() {
            unsafe {
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_RELOAD, w!("Reload config"));
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_OPEN, w!("Open config folder"));
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_DIAG, w!("Diagnose UIA (foreground)"));
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_ABOUT, w!("About"));
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_EXIT, w!("Quit"));
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetForegroundWindow(hwnd);
                let flags = TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
                let _ = TrackPopupMenu(hmenu, flags, pt.x, pt.y, Some(0), hwnd, None);
                let _ = DestroyMenu(hmenu);
            }
        }
        return LRESULT(0);
    }

    if msg == WM_COMMAND {
        let id = wparam.0 & 0xffff;
        if let Some(tx) = TRAY_TX.get() {
            let ev = match id {
                IDM_RELOAD => Some(TrayEvent::Reload),
                IDM_OPEN => Some(TrayEvent::OpenConfigFolder),
                IDM_DIAG => Some(TrayEvent::Diagnose),
                IDM_ABOUT => Some(TrayEvent::About),
                IDM_EXIT => Some(TrayEvent::Quit),
                _ => None,
            };
            if let Some(e) = ev {
                let _ = tx.send(e);
            }
        }
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Runs a message loop on a background thread; delivers [`TrayEvent`] on `tx`.
pub fn spawn(tx: Sender<TrayEvent>) {
    let _ = TRAY_TX.set(tx.clone());
    std::thread::Builder::new()
        .name("navigator-tray".into())
        .spawn(move || unsafe {
            run_tray_loop();
        })
        .expect("tray thread");
}

unsafe fn run_tray_loop() {
    let Ok(module) = GetModuleHandleW(None) else {
        return;
    };
    let instance = HINSTANCE(module.0);

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(tray_wndproc),
        hInstance: instance,
        lpszClassName: w!("Navigator.TrayHost.M10"),
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let Ok(hwnd) = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("Navigator.TrayHost.M10"),
        w!("Navigator tray"),
        WINDOW_STYLE::default(),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(instance),
        None,
    ) else {
        return;
    };

    let Ok(icon) = LoadIconW(None, IDI_APPLICATION) else {
        return;
    };

    let flags: NOTIFY_ICON_DATA_FLAGS =
        NOTIFY_ICON_DATA_FLAGS(NIF_MESSAGE.0 | NIF_ICON.0 | NIF_TIP.0);

    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = flags;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = icon;
    nid.szTip = wide_tip("Navigator");

    if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() } {
        return;
    }

    let mut msg = MSG::default();
    loop {
        let ret = GetMessageW(&mut msg, None, 0, 0);
        if !ret.as_bool() {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}
