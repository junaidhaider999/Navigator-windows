//! Layered popups: one borderless topmost window per display (C2: D2D + DirectComposition).

use crossbeam_channel::Receiver;
use nav_core::Hint;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT,
    RECT, RPC_E_CHANGED_MODE, WPARAM,
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
use crate::monitors::{enumerate_monitor_rects, physical_point_in_monitor_rect};

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

struct OverlaySlot {
    hwnd: HWND,
    /// `rcMonitor` in virtual screen coordinates (physical pixels).
    monitor: RECT,
    gpu: Option<D2dCompositionRenderer>,
    visible: bool,
}

impl OverlaySlot {
    unsafe fn destroy(self) {
        let _ = DestroyWindow(self.hwnd);
    }
}

struct OverlayStack {
    instance: HINSTANCE,
    slots: Vec<OverlaySlot>,
}

impl OverlayStack {
    unsafe fn new() -> Result<Self, RenderError> {
        let module: HMODULE =
            GetModuleHandleW(None).map_err(|e| RenderError::Win32(e.to_string()))?;
        let instance: HINSTANCE = module.into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: WNDCLASS_STYLES(CS_HREDRAW.0 | CS_VREDRAW.0),
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: instance,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            let err = unsafe { GetLastError() };
            if err != ERROR_CLASS_ALREADY_EXISTS {
                return Err(RenderError::Win32(format!(
                    "RegisterClassExW failed: {err:?}"
                )));
            }
        }
        Ok(Self {
            instance,
            slots: Vec::new(),
        })
    }

    /// Rebuild HWND + GPU slots to match the current monitor layout.
    unsafe fn sync_to_monitors(&mut self) -> Result<(), RenderError> {
        let rects = enumerate_monitor_rects()?;
        if rects.len() != self.slots.len() {
            for s in self.slots.drain(..) {
                s.destroy();
            }
            for r in &rects {
                let hwnd = Self::create_overlay_hwnd(self.instance, *r)?;
                let gpu = Some(D2dCompositionRenderer::new(hwnd)?);
                self.slots.push(OverlaySlot {
                    hwnd,
                    monitor: *r,
                    gpu,
                    visible: false,
                });
            }
            return Ok(());
        }
        for (slot, r) in self.slots.iter_mut().zip(rects.iter()) {
            slot.monitor = *r;
            let w = r.right - r.left;
            let h = r.bottom - r.top;
            SetWindowPos(
                slot.hwnd,
                Some(HWND_TOPMOST),
                r.left,
                r.top,
                w,
                h,
                SWP_NOACTIVATE,
            )
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        }
        Ok(())
    }

    unsafe fn create_overlay_hwnd(instance: HINSTANCE, area: RECT) -> Result<HWND, RenderError> {
        let w = area.right - area.left;
        let h = area.bottom - area.top;
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
            Some(instance),
            None,
        )
        .map_err(|e| RenderError::Win32(e.to_string()))?;
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
            .map_err(|e| RenderError::Win32(format!("SetLayeredWindowAttributes: {e}")))?;
        Ok(hwnd)
    }

    unsafe fn pump_all_hwnds(&self) {
        for s in &self.slots {
            pump_messages(s.hwnd);
        }
    }

    unsafe fn hide_all(&mut self) {
        for s in &mut self.slots {
            s.visible = false;
            let _ = ShowWindow(s.hwnd, SW_HIDE);
        }
    }

    unsafe fn teardown(mut self) {
        self.hide_all();
        for mut s in self.slots.drain(..) {
            drop(s.gpu.take());
            s.destroy();
        }
        pump_all_thread_messages();
        let _ = UnregisterClassW(CLASS_NAME, Some(self.instance));
    }
}

fn partition_hints(hints: &[Hint], monitors: &[RECT]) -> Vec<Vec<Hint>> {
    let mut out: Vec<Vec<Hint>> = (0..monitors.len()).map(|_| Vec::new()).collect();
    for h in hints {
        let x = h.raw.bounds.x;
        let y = h.raw.bounds.y;
        let idx = monitors
            .iter()
            .position(|r| physical_point_in_monitor_rect(x, y, r))
            .unwrap_or(0);
        out[idx].push(h.clone());
    }
    out
}

pub fn run_render_thread(cmd_rx: Receiver<RenderCmd>) {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        eprintln!("[render] CoInitializeEx: {hr:?}");
        return;
    }

    let mut stack: Option<OverlayStack> = None;
    let mut max_show_accepted: u64 = 0;
    let mut displayed_session: Option<u64> = None;

    loop {
        let cmd = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => RenderCmd::Shutdown,
        };

        match cmd {
            RenderCmd::Prewarm => {
                let res = (|| unsafe {
                    if stack.is_none() {
                        stack = Some(OverlayStack::new()?);
                    }
                    let st = stack.as_mut().unwrap();
                    st.sync_to_monitors()?;
                    for s in &mut st.slots {
                        if let Some(ref mut g) = s.gpu {
                            g.update_and_present(&[])?;
                        }
                        let _ = ShowWindow(s.hwnd, SW_HIDE);
                        s.visible = false;
                    }
                    Ok::<(), RenderError>(())
                })();
                match res {
                    Ok(()) => {}
                    Err(e) => eprintln!("[render] prewarm failed: {e}"),
                }
            }
            RenderCmd::Show { session_id, hints } => {
                if session_id <= max_show_accepted {
                    if let Some(ref st) = stack {
                        unsafe { st.pump_all_hwnds() };
                    }
                    continue;
                }
                let res = (|| unsafe {
                    if stack.is_none() {
                        stack = Some(OverlayStack::new()?);
                    }
                    let st = stack.as_mut().unwrap();
                    st.sync_to_monitors()?;
                    if st.slots.is_empty() {
                        return Err(RenderError::Win32("no overlay slots after sync".into()));
                    }
                    let monitors: Vec<RECT> = st.slots.iter().map(|s| s.monitor).collect();
                    let parts = partition_hints(&hints, &monitors);
                    for (s, part) in st.slots.iter_mut().zip(parts.iter()) {
                        if let Some(ref mut g) = s.gpu {
                            g.update_and_present(part)?;
                        }
                        if part.is_empty() {
                            let _ = ShowWindow(s.hwnd, SW_HIDE);
                            s.visible = false;
                        } else {
                            let _ = ShowWindow(s.hwnd, SW_SHOW);
                            s.visible = true;
                        }
                    }
                    Ok::<(), RenderError>(())
                })();
                match res {
                    Ok(()) => {
                        max_show_accepted = session_id;
                        displayed_session = Some(session_id);
                    }
                    Err(e) => eprintln!("[render] show failed: {e}"),
                }
            }
            RenderCmd::Repaint { session_id, hints } => {
                if displayed_session != Some(session_id) {
                    if let Some(ref st) = stack {
                        unsafe { st.pump_all_hwnds() };
                    }
                    continue;
                }
                let res = (|| unsafe {
                    let st = stack.as_mut().ok_or_else(|| {
                        RenderError::Win32("repaint with no overlay stack".into())
                    })?;
                    st.sync_to_monitors()?;
                    let monitors: Vec<RECT> = st.slots.iter().map(|s| s.monitor).collect();
                    let parts = partition_hints(&hints, &monitors);
                    for (s, part) in st.slots.iter_mut().zip(parts.iter()) {
                        if let Some(ref mut g) = s.gpu {
                            g.update_and_present(part)?;
                        }
                        if part.is_empty() {
                            let _ = ShowWindow(s.hwnd, SW_HIDE);
                            s.visible = false;
                        } else {
                            let _ = ShowWindow(s.hwnd, SW_SHOW);
                            s.visible = true;
                        }
                    }
                    Ok::<(), RenderError>(())
                })();
                if let Err(e) = res {
                    eprintln!("[render] repaint: {e}");
                }
            }
            RenderCmd::Hide { session_id } => {
                if displayed_session != Some(session_id) {
                    if let Some(ref st) = stack {
                        unsafe { st.pump_all_hwnds() };
                    }
                    continue;
                }
                displayed_session = None;
                if let Some(ref mut st) = stack {
                    unsafe { st.hide_all() };
                }
            }
            RenderCmd::Shutdown => {
                if let Some(st) = stack.take() {
                    unsafe { st.teardown() };
                }
                break;
            }
        }

        if let Some(ref st) = stack {
            unsafe { st.pump_all_hwnds() };
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
