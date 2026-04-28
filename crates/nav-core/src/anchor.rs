//! Heuristic anchor (physical px) when UIA does not supply a clickable point.

use crate::geom::Rect;
use crate::hint::ElementKind;

/// Fallback “truth” point inside `bounds` for pill placement / locality (physical screen pixels).
#[must_use]
pub fn fallback_anchor_px(bounds: Rect, kind: ElementKind) -> (i32, i32) {
    let w = bounds.w.max(1);
    let h = bounds.h.max(1);
    let cx = bounds.x + w / 2;
    let cy = bounds.y + h / 2;

    match kind {
        ElementKind::Editable => {
            let inset_x = (w / 8).clamp(2, 10);
            (bounds.x + inset_x + (w - 2 * inset_x) / 6, cy)
        }
        _ => {
            // Wide controls: bias toward central band (label / icon zone), matching invoke click heuristic.
            if w > h.max(16) * 3 || w > 120 {
                let strip = ((w as f32) * 0.6).round() as i32;
                let strip = strip.clamp(1, w);
                let sl = bounds.x + (w - strip) / 2;
                (sl + strip / 2, cy)
            } else {
                (cx, cy)
            }
        }
    }
}
