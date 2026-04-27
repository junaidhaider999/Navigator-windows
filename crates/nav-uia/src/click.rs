//! Physical left-click at screen coordinates (fallback when UIA patterns are insufficient).

use nav_core::Rect;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

use crate::UiaError;

/// Single left click at the center of `rect` (physical screen pixels, per UIA bounding rects).
pub fn left_click_rect_center(rect: &Rect) -> Result<(), UiaError> {
    let cx = rect.x + rect.w / 2;
    let cy = rect.y + rect.h / 2;
    unsafe {
        SetCursorPos(cx, cy).map_err(|e| UiaError::Operation(format!("SetCursorPos: {e}")))?;
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
    Ok(())
}
