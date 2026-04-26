//! Virtual-screen / monitor geometry for overlay placement.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};

use crate::RenderError;

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
