//! Last-resort `EnumChildWindows` enumeration (Phase E2).

use nav_core::{Backend, ElementKind, RawHint, Rect, fallback_anchor_px};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetWindowRect, IsWindowVisible};

use crate::UiaError;
use crate::hwnd::UiaHwnd;
use crate::options::EnumOptions;

struct CollectCtx {
    root: HWND,
    max: usize,
    out: Vec<RawHint>,
}

unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut CollectCtx) };
    if ctx.out.len() >= ctx.max {
        return BOOL(0);
    }
    if hwnd == ctx.root {
        return BOOL(1);
    }
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || !unsafe { IsWindowEnabled(hwnd) }.as_bool() {
        return BOOL(1);
    }

    let mut wr = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut wr) }.is_err() {
        return BOOL(1);
    }
    let w = wr.right.saturating_sub(wr.left);
    let h = wr.bottom.saturating_sub(wr.top);
    if w < 4 || h < 4 {
        return BOOL(1);
    }

    let id = ctx.out.len() as u64;
    let b = Rect {
            x: wr.left,
            y: wr.top,
            w,
            h,
        };
    let anchor = fallback_anchor_px(b, ElementKind::GenericClickable);
    ctx.out.push(RawHint {
        element_id: id,
        uia_runtime_id_fp: None,
        uia_invoke_hwnd: Some(hwnd.0 as usize),
        uia_child_index: None,
        bounds: b,
        anchor_px: Some(anchor),
        kind: ElementKind::GenericClickable,
        name: None,
        backend: Backend::RawHwnd,
    });
    BOOL(1)
}

/// HWND-only enumeration for `root` (children only; `root` is not listed).
pub fn enumerate_raw_hwnd(root: UiaHwnd, opts: &EnumOptions) -> Result<Vec<RawHint>, UiaError> {
    if root.is_invalid() {
        return Ok(Vec::new());
    }
    let mut ctx = CollectCtx {
        root,
        max: opts.max_elements,
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumChildWindows(
            Some(root),
            Some(enum_child_proc),
            LPARAM(&mut ctx as *mut CollectCtx as isize),
        );
    }
    Ok(ctx.out)
}
