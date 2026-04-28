//! Physical left-click at screen coordinates (fallback when UIA patterns are insufficient).

use nav_core::{RawHint, Rect, fallback_anchor_px};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, mouse_event,
};
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

use crate::UiaError;

/// Clamp `(x, y)` into `rect` (physical pixels).
#[must_use]
pub fn clamp_point_to_bounds(x: i32, y: i32, rect: &Rect) -> (i32, i32) {
    if rect.w <= 0 || rect.h <= 0 {
        return (rect.x, rect.y);
    }
    let max_x = rect.x.saturating_add(rect.w.saturating_sub(1));
    let max_y = rect.y.saturating_add(rect.h.saturating_sub(1));
    (x.clamp(rect.x, max_x), y.clamp(rect.y, max_y))
}

/// Prefer UIA [`RawHint::anchor_px`], else [`fallback_anchor_px`], clamped to bounds.
#[must_use]
pub fn resolve_invoke_physical_point(raw: &RawHint) -> (i32, i32) {
    let r = &raw.bounds;
    let (x, y) = raw
        .anchor_px
        .unwrap_or_else(|| fallback_anchor_px(*r, raw.kind));
    clamp_point_to_bounds(x, y, r)
}

/// Physical click at the resolved invoke point for this hint (matches overlay anchor).
pub fn invoke_click_hint(raw: &RawHint) -> Result<(), UiaError> {
    let r = &raw.bounds;
    if r.w <= 0 || r.h <= 0 {
        return Err(UiaError::Operation("zero-size bounds for click".into()));
    }
    let (cx, cy) = resolve_invoke_physical_point(raw);
    unsafe {
        SetCursorPos(cx, cy).map_err(|e| UiaError::Operation(format!("SetCursorPos: {e}")))?;
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
    Ok(())
}

/// Best-effort point inside `rect` for a physical click: inset from edges, wide controls biased
/// toward the “text” band, tiny controls use the center. Zero-size bounds return an error.
pub fn invoke_click_point(rect: &Rect) -> Result<(), UiaError> {
    if rect.w <= 0 || rect.h <= 0 {
        return Err(UiaError::Operation("zero-size bounds for click".into()));
    }
    let (cx, cy) = best_click_point(rect);
    unsafe {
        SetCursorPos(cx, cy).map_err(|e| UiaError::Operation(format!("SetCursorPos: {e}")))?;
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }
    Ok(())
}

/// [`Rect`] center (legacy). Prefer [`invoke_click_point`].
#[allow(dead_code)] // Kept for API compatibility; new code should use `invoke_click_point`.
pub fn left_click_rect_center(rect: &Rect) -> Result<(), UiaError> {
    invoke_click_point(rect)
}

/// Screen pixel (x, y) for `SendInput` / `ElementFromPoint` resolution.
pub fn best_click_point(rect: &Rect) -> (i32, i32) {
    let w = rect.w;
    let h = rect.h;
    if w <= 0 || h <= 0 {
        return (rect.x, rect.y);
    }

    // Tiny: center of the rect (no inset — border is negligible).
    if w <= 4 || h <= 4 {
        return (rect.x + w / 2, rect.y + h / 2);
    }

    let inset_x = ((w / 8).clamp(1, 4)).min(w / 3);
    let inset_y = ((h / 8).clamp(1, 4)).min(h / 3);
    let inner_w = (w - 2 * inset_x).max(1);
    let inner_h = (h - 2 * inset_y).max(1);
    let inner_left = rect.x + inset_x;
    let inner_top = rect.y + inset_y;

    let cx = if w > h.max(16) * 3 || w > 120 {
        // Wide control: bias toward central band (approximate “text center”).
        let strip = ((w as f32) * 0.6).round() as i32;
        let strip = strip.clamp(1, w);
        let sl = rect.x + (w - strip) / 2;
        sl + strip / 2
    } else {
        inner_left + inner_w / 2
    };
    let cy = inner_top + inner_h / 2;
    (cx, cy)
}
