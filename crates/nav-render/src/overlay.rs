//! Layered popup covering the primary monitor (C1).

use std::time::Duration;

use crossbeam_channel::{Receiver, select, tick};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{HBITMAP, HDC, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassExW, SW_HIDE,
    SW_SHOW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, TranslateMessage,
    ULW_ALPHA, UnregisterClassW, UpdateLayeredWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY,
    WNDCLASS_STYLES, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};

use crate::RenderError;
use crate::device::{create_layer_dc, destroy_layer_dc};
use crate::monitors::primary_monitor_rect;

const CLASS_NAME: PCWSTR = w!("Navigator.RenderOverlay.C1");

/// `session_id` is forwarded from the app for future staleness checks; C1 only toggles visibility.
#[allow(dead_code)]
pub(crate) enum RenderCmd {
    Show { session_id: u64 },
    Hide { session_id: u64 },
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
    let ticker = tick(Duration::from_millis(32));
    let mut hwnd: Option<HWND> = None;
    let mut visible = false;
    let mut mem_dc: Option<HDC> = None;
    let mut dib: Option<HBITMAP> = None;
    let mut old_gdi: Option<HGDIOBJ> = None;
    let mut instance: Option<HINSTANCE> = None;

    'outer: loop {
        select! {
            recv(cmd_rx) -> cmd => {
                match cmd {
                    Ok(RenderCmd::Show { session_id: _ }) => {
                        if let Err(e) = unsafe { show_overlay(&mut hwnd, &mut instance, &mut mem_dc, &mut dib, &mut old_gdi, &mut visible) } {
                            eprintln!("[render] show failed: {e}");
                        }
                    }
                    Ok(RenderCmd::Hide { session_id: _ }) => {
                        unsafe { hide_overlay(&mut hwnd, &mut mem_dc, &mut dib, &mut old_gdi, &mut visible) };
                    }
                    Ok(RenderCmd::Shutdown) | Err(_) => {
                        unsafe { hide_overlay(&mut hwnd, &mut mem_dc, &mut dib, &mut old_gdi, &mut visible) };
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
                    if visible && escape_pressed() {
                        unsafe { hide_overlay(&mut hwnd, &mut mem_dc, &mut dib, &mut old_gdi, &mut visible) };
                    }
                }
            }
        }
    }
}

fn escape_pressed() -> bool {
    unsafe { (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 }
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

unsafe fn release_layer(
    mem_dc: &mut Option<HDC>,
    dib: &mut Option<HBITMAP>,
    old_gdi: &mut Option<HGDIOBJ>,
) {
    if let (Some(dc), Some(bmp), Some(old)) = (mem_dc.take(), dib.take(), old_gdi.take()) {
        destroy_layer_dc(dc, bmp, old);
    }
}

unsafe fn show_overlay(
    hwnd_slot: &mut Option<HWND>,
    instance_slot: &mut Option<HINSTANCE>,
    mem_dc: &mut Option<HDC>,
    dib: &mut Option<HBITMAP>,
    old_gdi: &mut Option<HGDIOBJ>,
    visible: &mut bool,
) -> Result<(), RenderError> {
    release_layer(mem_dc, dib, old_gdi);

    let area = primary_monitor_rect()?;
    let w = area.right - area.left;
    let h = area.bottom - area.top;

    let (dc, bmp, old) = create_layer_dc(area)?;
    *mem_dc = Some(dc);
    *dib = Some(bmp);
    *old_gdi = Some(old);

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
            release_layer(mem_dc, dib, old_gdi);
            let err = windows::Win32::Foundation::GetLastError();
            return Err(RenderError::Win32(format!(
                "RegisterClassExW failed: {err:?}"
            )));
        }

        let exstyle = WINDOW_EX_STYLE(
            WS_EX_LAYERED.0
                | WS_EX_TRANSPARENT.0
                | WS_EX_TOPMOST.0
                | WS_EX_NOACTIVATE.0
                | WS_EX_TOOLWINDOW.0
                | WS_EX_NOREDIRECTIONBITMAP.0,
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
        .map_err(|e| {
            release_layer(mem_dc, dib, old_gdi);
            RenderError::Win32(e.to_string())
        })?;

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
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    )
    .map_err(|e| RenderError::Win32(e.to_string()))?;

    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };

    let dst_pt = POINT {
        x: area.left,
        y: area.top,
    };
    let size = SIZE { cx: w, cy: h };
    let src_pt = POINT { x: 0, y: 0 };

    let mdc = mem_dc
        .as_ref()
        .ok_or_else(|| RenderError::Win32("internal: missing mem DC".into()))?;

    UpdateLayeredWindow(
        hwnd,
        None,
        Some(&dst_pt),
        Some(&size),
        Some(*mdc),
        Some(&src_pt),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    )
    .map_err(|e| RenderError::Win32(e.to_string()))?;

    let _ = ShowWindow(hwnd, SW_SHOW);
    *visible = true;
    Ok(())
}

unsafe fn hide_overlay(
    hwnd_slot: &mut Option<HWND>,
    mem_dc: &mut Option<HDC>,
    dib: &mut Option<HBITMAP>,
    old_gdi: &mut Option<HGDIOBJ>,
    visible: &mut bool,
) {
    *visible = false;
    if let Some(h) = *hwnd_slot {
        let _ = ShowWindow(h, SW_HIDE);
    }
    release_layer(mem_dc, dib, old_gdi);
}
