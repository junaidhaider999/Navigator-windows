//! Layered popup covering the primary monitor (C2: D2D + DirectComposition).

use std::time::Duration;

use crossbeam_channel::{Receiver, select, tick};
use nav_core::Hint;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, RPC_E_CHANGED_MODE, WPARAM,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassExW,
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WNDCLASS_STYLES,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::RenderError;
use crate::d2d::D2dCompositionRenderer;
use crate::monitors::primary_monitor_rect;

const CLASS_NAME: PCWSTR = w!("Navigator.RenderOverlay.C2");

pub(crate) enum RenderCmd {
    /// Create hidden overlay HWND + D3D/D2D/DComp once at app boot (D2).
    Prewarm,
    Show {
        session_id: u64,
        hints: Vec<Hint>,
    },
    Repaint {
        session_id: u64,
        hints: Vec<Hint>,
    },
    Hide {
        session_id: u64,
    },
    Shutdown,
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn run_render_thread(cmd_rx: Receiver<RenderCmd>) {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        eprintln!("[render] CoInitializeEx: {hr:?}");
        return;
    }

    let ticker = tick(Duration::from_millis(32));
    let mut hwnd: Option<HWND> = None;
    let mut visible = false;
    let mut instance: Option<HINSTANCE> = None;
    let mut gpu: Option<D2dCompositionRenderer> = None;
    // Highest session_id accepted from RenderCmd::Show (app uses monotonic ids).
    let mut max_show_accepted: u64 = 0;
    // Session id currently shown on the swap chain, if any.
    let mut displayed_session: Option<u64> = None;
    let mut pending_hints: Vec<Hint> = Vec::new();

    'outer: loop {
        select! {
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(RenderCmd::Prewarm) => {
                        match unsafe { prewarm_overlay(&mut hwnd, &mut instance, &mut gpu, &mut visible) }
                        {
                            Ok(()) => {}
                            Err(e) => eprintln!("[render] prewarm failed: {e}"),
                        }
                    }
                    Ok(RenderCmd::Show { session_id, hints }) => {
                        if session_id <= max_show_accepted {
                            continue;
                        }
                        match unsafe {
                            show_overlay(
                                &mut hwnd,
                                &mut instance,
                                &mut gpu,
                                &mut visible,
                                &hints,
                            )
                        } {
                            Ok(()) => {
                                max_show_accepted = session_id;
                                displayed_session = Some(session_id);
                                pending_hints = hints;
                            }
                            Err(e) => eprintln!("[render] show failed: {e}"),
                        }
                    }
                    Ok(RenderCmd::Repaint { session_id, hints }) => {
                        if displayed_session != Some(session_id) {
                            continue;
                        }
                        pending_hints = hints;
                    }
                    Ok(RenderCmd::Hide { session_id }) => {
                        if displayed_session != Some(session_id) {
                            continue;
                        }
                        unsafe { hide_overlay(&mut hwnd, &mut gpu, &mut visible, &mut displayed_session) };
                    }
                    Ok(RenderCmd::Shutdown) | Err(_) => {
                        unsafe { hide_overlay(&mut hwnd, &mut gpu, &mut visible, &mut displayed_session) };
                        drop(gpu.take());
                        if let (Some(h), Some(inst)) = (hwnd.take(), instance.take()) {
                            unsafe {
                                let _ = DestroyWindow(h);
                            }
                            pump_all_thread_messages();
                            unsafe {
                                let _ = UnregisterClassW(CLASS_NAME, Some(inst));
                            }
                        }
                        break 'outer;
                    }
                }
            }
            recv(ticker) -> _ => {
                if let Some(h) = hwnd {
                    unsafe { pump_messages(h) };
                    if visible {
                        if let Some(ref mut g) = gpu {
                            if let Err(e) = unsafe { g.update_and_present(&pending_hints) } {
                                eprintln!("[render] frame: {e}");
                            }
                        }
                    }
                }
            }
        }
    }

    unsafe { CoUninitialize() };
}

unsafe fn pump_messages(hwnd: HWND) {
    let mut msg = MSG::default();
    while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

fn pump_all_thread_messages() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Creates the overlay HWND (once), positions it on the primary monitor, and sets layered alpha.
/// Does not show the window or touch the GPU stack.
unsafe fn prepare_overlay_surface(
    hwnd_slot: &mut Option<HWND>,
    instance_slot: &mut Option<HINSTANCE>,
) -> Result<HWND, RenderError> {
    let area = primary_monitor_rect()?;
    let w = area.right - area.left;
    let h = area.bottom - area.top;

    let module: HMODULE = GetModuleHandleW(None).map_err(|e| RenderError::Win32(e.to_string()))?;
    let inst: HINSTANCE = module.into();
    *instance_slot = Some(inst);

    let hwnd = if let Some(hwnd) = *hwnd_slot {
        hwnd
    } else {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: WNDCLASS_STYLES(CS_HREDRAW.0 | CS_VREDRAW.0),
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: inst,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            let err = windows::Win32::Foundation::GetLastError();
            return Err(RenderError::Win32(format!(
                "RegisterClassExW failed: {err:?}"
            )));
        }

        // Omit `WS_EX_NOREDIRECTIONBITMAP`: with `CreateSwapChainForHwnd` + layered popups it can
        // trigger `DXGI_ERROR_INVALID_CALL` on several driver stacks (see C3 overlay bring-up).
        let exstyle = WINDOW_EX_STYLE(
            WS_EX_LAYERED.0
                | WS_EX_TRANSPARENT.0
                | WS_EX_TOPMOST.0
                | WS_EX_NOACTIVATE.0
                | WS_EX_TOOLWINDOW.0,
        );

        let hwnd = CreateWindowExW(
            exstyle,
            CLASS_NAME,
            PCWSTR::null(),
            WINDOW_STYLE(WS_POPUP.0),
            area.left,
            area.top,
            w,
            h,
            None,
            None,
            Some(inst),
            None,
        )
        .map_err(|e| RenderError::Win32(e.to_string()))?;

        *hwnd_slot = Some(hwnd);
        hwnd
    };

    SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        area.left,
        area.top,
        w,
        h,
        SWP_NOACTIVATE,
    )
    .map_err(|e| RenderError::Win32(e.to_string()))?;

    SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
        .map_err(|e| RenderError::Win32(format!("SetLayeredWindowAttributes: {e}")))?;

    Ok(hwnd)
}

unsafe fn prewarm_overlay(
    hwnd_slot: &mut Option<HWND>,
    instance_slot: &mut Option<HINSTANCE>,
    gpu: &mut Option<D2dCompositionRenderer>,
    visible: &mut bool,
) -> Result<(), RenderError> {
    let hwnd = prepare_overlay_surface(hwnd_slot, instance_slot)?;
    if gpu.is_none() {
        *gpu = Some(D2dCompositionRenderer::new(hwnd)?);
    }
    if let Some(ref mut g) = *gpu {
        g.update_and_present(&[])?;
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
    *visible = false;
    Ok(())
}

unsafe fn show_overlay(
    hwnd_slot: &mut Option<HWND>,
    instance_slot: &mut Option<HINSTANCE>,
    gpu: &mut Option<D2dCompositionRenderer>,
    visible: &mut bool,
    hints: &[Hint],
) -> Result<(), RenderError> {
    let hwnd = prepare_overlay_surface(hwnd_slot, instance_slot)?;
    if gpu.is_none() {
        *gpu = Some(D2dCompositionRenderer::new(hwnd)?);
    }
    if let Some(ref mut g) = *gpu {
        g.update_and_present(hints)?;
    }
    let _ = ShowWindow(hwnd, SW_SHOW);
    *visible = true;
    Ok(())
}

unsafe fn hide_overlay(
    hwnd_slot: &mut Option<HWND>,
    _gpu: &mut Option<D2dCompositionRenderer>,
    visible: &mut bool,
    displayed_session: &mut Option<u64>,
) {
    *visible = false;
    *displayed_session = None;
    // Keep D3D/D2D/DComp alive between sessions (D2 pre-warm).
    if let Some(h) = *hwnd_slot {
        let _ = ShowWindow(h, SW_HIDE);
    }
}
