//! Virtual-screen / monitor geometry for overlay placement.

use windows::Win32::Foundation::{BOOL, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, PtInRect,
};

use crate::RenderError;

unsafe extern "system" fn monitor_enum_proc(
    hmon: HMONITOR,
    _: HDC,
    _lprc: *mut RECT,
    data: LPARAM,
) -> BOOL {
    if data.0 == 0 {
        return TRUE;
    }
    let vec = unsafe { &mut *(data.0 as *mut Vec<RECT>) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        vec.push(mi.rcMonitor);
    }
    TRUE
}

/// All `rcMonitor` rectangles in virtual screen coordinates, in enumeration order.
pub fn enumerate_monitor_rects() -> Result<Vec<RECT>, RenderError> {
    let mut out = Vec::new();
    let ptr = &mut out as *mut Vec<RECT>;
    let ok =
        unsafe { EnumDisplayMonitors(None, None, Some(monitor_enum_proc), LPARAM(ptr as isize)) };
    if ok.as_bool() && !out.is_empty() {
        return Ok(out);
    }
    Ok(vec![primary_monitor_rect()?])
}

/// True if `(x, y)` in virtual screen pixels lies inside `r` (uses `PtInRect`).
#[must_use]
pub fn physical_point_in_monitor_rect(x: i32, y: i32, r: &RECT) -> bool {
    let pt = POINT { x, y };
    unsafe { PtInRect(std::ptr::from_ref(r), pt).as_bool() }
}

/// Primary monitor rectangle in **physical screen coordinates** (`rcMonitor`).
pub fn primary_monitor_rect() -> Result<RECT, RenderError> {
    let pt = POINT { x: 1, y: 1 };
    let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY) };
    if hmon.is_invalid() {
        return Err(RenderError::Win32("MonitorFromPoint returned null".into()));
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        return Err(RenderError::Win32(format!(
            "GetMonitorInfoW failed: {err:?}"
        )));
    }
    Ok(mi.rcMonitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_monitor_rect_is_non_empty() {
        let r = primary_monitor_rect().expect("primary_monitor_rect");
        assert!(r.right > r.left && r.bottom > r.top);
    }

    #[test]
    fn enumerate_monitor_rects_non_empty() {
        let v = enumerate_monitor_rects().expect("enumerate_monitor_rects");
        assert!(!v.is_empty());
    }
}
