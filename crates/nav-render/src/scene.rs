//! Layout and drawing for overlay “hint pills” (C3: real [`nav_core::Hint`] bounds → client DIPs).
//! Frame classification is NoOp vs full redraw only: partial clips are unsound under
//! `DXGI_SWAP_EFFECT_FLIP_DISCARD` (back-buffer contents undefined after `Present`).

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use nav_core::Hint;
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ROUNDED_RECT, ID2D1DeviceContext,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, IDWriteFactory,
    IDWriteTextFormat,
};

use crate::RenderError;

/// One rounded pill with a UTF-8 label.
#[derive(Clone, Debug, PartialEq)]
pub struct PillGeom {
    pub rect: D2D_RECT_F,
    pub label: String,
}

/// Whether this frame can skip work or must clear and redraw all pills.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintPlan {
    /// Same pill geometry as last present; skip `BeginDraw` / `Present`.
    NoOp,
    /// Full clear + draw all pills.
    Full,
}

fn duplicate_label(pills: &[PillGeom]) -> bool {
    let mut seen = HashSet::with_capacity(pills.len());
    for p in pills {
        if !seen.insert(p.label.as_str()) {
            return true;
        }
    }
    false
}

fn rects_equal(a: D2D_RECT_F, b: D2D_RECT_F) -> bool {
    (a.left - b.left).abs() <= f32::EPSILON
        && (a.top - b.top).abs() <= f32::EPSILON
        && (a.right - b.right).abs() <= f32::EPSILON
        && (a.bottom - b.bottom).abs() <= f32::EPSILON
}

/// Same set of (label, rect) pairs as last frame (order-independent); used to skip identical paints.
pub fn pills_geometrically_equal(a: &[PillGeom], b: &[PillGeom]) -> bool {
    if duplicate_label(a) || duplicate_label(b) {
        return false;
    }
    if a.len() != b.len() {
        return false;
    }
    let mut ma: HashMap<&str, D2D_RECT_F> = HashMap::with_capacity(a.len());
    for p in a {
        ma.insert(p.label.as_str(), p.rect);
    }
    let mut mb: HashMap<&str, D2D_RECT_F> = HashMap::with_capacity(b.len());
    for p in b {
        mb.insert(p.label.as_str(), p.rect);
    }
    if ma.len() != mb.len() {
        return false;
    }
    for (k, ra) in &ma {
        match mb.get(k) {
            Some(rb) if rects_equal(*ra, *rb) => {}
            _ => return false,
        }
    }
    true
}

/// Decide no-op vs full redraw (`client_w` / `client_h` kept for a stable API).
#[must_use]
pub fn paint_plan(old: &[PillGeom], new: &[PillGeom], client_w: f32, client_h: f32) -> PaintPlan {
    let _ = (client_w, client_h);
    if pills_geometrically_equal(old, new) {
        PaintPlan::NoOp
    } else {
        PaintPlan::Full
    }
}

const MIN_PILL_W: f32 = 72.0;
const MIN_PILL_H: f32 = 28.0;
const MAX_PILL_W: f32 = 200.0;
const MAX_PILL_H: f32 = 80.0;

/// `overlay_origin_phys` is the overlay HWND top-left in physical screen pixels (matches UIA bounds).
pub fn pills_for_frame(
    hints: &[Hint],
    overlay_origin_phys: (i32, i32),
    client_w: f32,
    client_h: f32,
    dpi: f32,
) -> Vec<PillGeom> {
    if hints.is_empty() {
        return Vec::new();
    }
    let (ox, oy) = overlay_origin_phys;
    let scale = 96.0 / dpi;
    let mut out = Vec::with_capacity(hints.len());
    for h in hints {
        let left = (h.raw.bounds.x - ox) as f32 * scale;
        let top = (h.raw.bounds.y - oy) as f32 * scale;
        let w = h.raw.bounds.w as f32 * scale;
        let hgt = h.raw.bounds.h as f32 * scale;
        let cx = left + w * 0.5;
        let cy = top + hgt * 0.5;
        let pw = w.clamp(MIN_PILL_W, MAX_PILL_W);
        let ph = hgt.clamp(MIN_PILL_H, MAX_PILL_H);
        let mut rect = D2D_RECT_F {
            left: cx - pw * 0.5,
            top: cy - ph * 0.5,
            right: cx + pw * 0.5,
            bottom: cy + ph * 0.5,
        };
        rect.left = rect.left.max(0.0);
        rect.top = rect.top.max(0.0);
        rect.right = rect.right.min(client_w);
        rect.bottom = rect.bottom.min(client_h);
        if rect.right > rect.left && rect.bottom > rect.top {
            out.push(PillGeom {
                rect,
                label: h.label.to_string(),
            });
        }
    }
    out
}

const CORNER_RADIUS: f32 = 8.0;

/// Fills rounded pills and draws centered labels. Call inside `BeginDraw`/`EndDraw`.
pub unsafe fn draw_pills(
    dc: &ID2D1DeviceContext,
    text_format: &IDWriteTextFormat,
    write: &IDWriteFactory,
    pills: &[PillGeom],
    fill: &ID2D1SolidColorBrush,
    border: &ID2D1SolidColorBrush,
    text_brush: &ID2D1SolidColorBrush,
    stroke: &ID2D1StrokeStyle,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    let opts = D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;

    for pill in pills {
        let rr = D2D1_ROUNDED_RECT {
            rect: pill.rect,
            radiusX: CORNER_RADIUS,
            radiusY: CORNER_RADIUS,
        };
        dc.FillRoundedRectangle(&rr, fill);
        dc.DrawRoundedRectangle(&rr, border, 1.5, stroke);

        let wlabel: Vec<u16> = pill.label.encode_utf16().collect();
        let layout = write
            .CreateTextLayout(
                &wlabel,
                text_format,
                (pill.rect.right - pill.rect.left).max(1.0),
                (pill.rect.bottom - pill.rect.top).max(1.0),
            )
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        layout
            .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        layout
            .SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        dc.DrawTextLayout(
            D2D_POINT_2F {
                x: pill.rect.left,
                y: pill.rect.top,
            },
            &layout,
            text_brush,
            opts,
        );
    }
    Ok(())
}

/// Premultiplied translucent navy fill (readable on arbitrary backgrounds).
pub fn pill_fill_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.12,
        g: 0.35,
        b: 0.78,
        a: 0.92,
    }
}

pub fn pill_border_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 0.95,
    }
}

pub fn pill_text_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pill(label: &str, rect: D2D_RECT_F) -> PillGeom {
        PillGeom {
            rect,
            label: label.into(),
        }
    }

    #[test]
    fn paint_plan_no_op_when_geometrically_identical() {
        let p = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
            },
        )];
        assert_eq!(paint_plan(&p, &p, 100.0, 100.0), PaintPlan::NoOp);
    }

    #[test]
    fn paint_plan_full_when_one_label_removed_far_apart() {
        let old = vec![
            pill(
                "aa",
                D2D_RECT_F {
                    left: 10.0,
                    top: 10.0,
                    right: 80.0,
                    bottom: 40.0,
                },
            ),
            pill(
                "ab",
                D2D_RECT_F {
                    left: 200.0,
                    top: 10.0,
                    right: 270.0,
                    bottom: 40.0,
                },
            ),
        ];
        let new = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 10.0,
                top: 10.0,
                right: 80.0,
                bottom: 40.0,
            },
        )];
        assert_eq!(paint_plan(&old, &new, 400.0, 300.0), PaintPlan::Full);
    }

    #[test]
    fn paint_plan_full_when_clearing_all_pills() {
        let old = vec![pill(
            "aa",
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 10.0,
                bottom: 10.0,
            },
        )];
        let new: Vec<PillGeom> = vec![];
        assert_eq!(paint_plan(&old, &new, 100.0, 100.0), PaintPlan::Full);
    }
}
