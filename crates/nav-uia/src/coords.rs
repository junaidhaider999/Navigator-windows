//! Map UIA bounding rectangles into [`nav_core::Rect`].

use nav_core::Rect;
use windows::Win32::Foundation::RECT;

/// Converts UIA `CurrentBoundingRectangle` (`RECT` with left/top/right/bottom) to physical-pixel `Rect`.
pub fn rect_from_uia_bounds(r: RECT) -> Option<Rect> {
    let w = r.right.saturating_sub(r.left);
    let h = r.bottom.saturating_sub(r.top);
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(Rect {
        x: r.left,
        y: r.top,
        w,
        h,
    })
}
