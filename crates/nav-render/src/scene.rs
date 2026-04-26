//! Layout and drawing for overlay “hint pills” (C3: real [`nav_core::Hint`] bounds → client DIPs).
//! D4: classify frame updates so filter-mode repaints can clip to dirty DIPs instead of full clears.

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

/// How much to inflate pill bounds when testing overlap with a dirty clip (stroke + AA slack).
pub const PILL_PAINT_SLOP_DIPS: f32 = 6.0;

/// How much the dirty region is grown before clipping (matches slop so clears cover removed ink).
const DIRTY_CLIP_PAD_DIPS: f32 = 6.0;

/// If the dirty region covers more than this fraction of the client, do a full-frame paint.
const PARTIAL_TO_FULL_AREA_FRAC: f32 = 0.52;

/// Whether this frame can skip work, must clear fully, or may clip to `clip_dips`.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintPlan {
    /// Same pill geometry as last present; skip `BeginDraw` / `Present`.
    NoOp,
    /// Full clear + draw all pills (first frame, resize, huge diffs, or ambiguous labels).
    Full,
    /// `PushAxisAlignedClip(clip_dips)` then erase + redraw only pills returned by
    /// [`pills_for_partial_repaint`](pills_for_partial_repaint).
    Partial { clip_dips: D2D_RECT_F },
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

fn rect_area(r: D2D_RECT_F) -> f32 {
    (r.right - r.left).max(0.0) * (r.bottom - r.top).max(0.0)
}

fn union_rect(a: D2D_RECT_F, b: D2D_RECT_F) -> D2D_RECT_F {
    D2D_RECT_F {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn expand_rect(r: D2D_RECT_F, pad: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.left - pad,
        top: r.top - pad,
        right: r.right + pad,
        bottom: r.bottom + pad,
    }
}

fn clamp_rect_to_client(mut r: D2D_RECT_F, cw: f32, ch: f32) -> D2D_RECT_F {
    r.left = r.left.max(0.0);
    r.top = r.top.max(0.0);
    r.right = r.right.min(cw);
    r.bottom = r.bottom.min(ch);
    r
}

fn rect_is_valid(r: &D2D_RECT_F) -> bool {
    r.right > r.left && r.bottom > r.top
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

fn dirty_union_from_label_diff(old: &[PillGeom], new: &[PillGeom]) -> Option<D2D_RECT_F> {
    let mut old_m: HashMap<&str, D2D_RECT_F> = HashMap::with_capacity(old.len());
    for p in old {
        old_m.insert(p.label.as_str(), p.rect);
    }
    let mut new_m: HashMap<&str, D2D_RECT_F> = HashMap::with_capacity(new.len());
    for p in new {
        new_m.insert(p.label.as_str(), p.rect);
    }
    let keys: HashSet<&str> = old_m.keys().chain(new_m.keys()).copied().collect();
    let mut dirty: Option<D2D_RECT_F> = None;
    for k in keys {
        let o = old_m.get(k).copied();
        let n = new_m.get(k).copied();
        match (o, n) {
            (Some(or), Some(nr)) if rects_equal(or, nr) => {}
            (Some(or), Some(nr)) => {
                let u = union_rect(or, nr);
                dirty = Some(match dirty {
                    None => u,
                    Some(d) => union_rect(d, u),
                });
            }
            (Some(or), None) => {
                dirty = Some(match dirty {
                    None => or,
                    Some(d) => union_rect(d, or),
                });
            }
            (None, Some(nr)) => {
                dirty = Some(match dirty {
                    None => nr,
                    Some(d) => union_rect(d, nr),
                });
            }
            (None, None) => {}
        }
    }
    dirty
}

/// Decide full vs partial vs no-op repaint (D4).
#[must_use]
pub fn paint_plan(old: &[PillGeom], new: &[PillGeom], client_w: f32, client_h: f32) -> PaintPlan {
    if pills_geometrically_equal(old, new) {
        return PaintPlan::NoOp;
    }
    if duplicate_label(old) || duplicate_label(new) {
        return PaintPlan::Full;
    }
    // Cleared overlay or first paint: need a global clear.
    if new.is_empty() || old.is_empty() {
        return PaintPlan::Full;
    }

    let Some(dirty) = dirty_union_from_label_diff(old, new) else {
        return PaintPlan::Full;
    };
    let dirty = expand_rect(dirty, DIRTY_CLIP_PAD_DIPS);
    let dirty = clamp_rect_to_client(dirty, client_w, client_h);
    if !rect_is_valid(&dirty) {
        return PaintPlan::Full;
    }

    let frame_area = (client_w * client_h).max(1.0);
    if rect_area(dirty) > PARTIAL_TO_FULL_AREA_FRAC * frame_area {
        return PaintPlan::Full;
    }

    PaintPlan::Partial { clip_dips: dirty }
}

/// Pills that may have visible ink inside `clip` after inflating by [`PILL_PAINT_SLOP_DIPS`].
#[must_use]
pub fn pills_for_partial_repaint(all: &[PillGeom], clip: &D2D_RECT_F) -> Vec<PillGeom> {
    all.iter()
        .filter(|p| {
            let r = expand_rect(p.rect, PILL_PAINT_SLOP_DIPS);
            r.left < clip.right && r.right > clip.left && r.top < clip.bottom && r.bottom > clip.top
        })
        .cloned()
        .collect()
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
    fn paint_plan_partial_when_one_label_removed_far_apart() {
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
        assert!(matches!(
            paint_plan(&old, &new, 400.0, 300.0),
            PaintPlan::Partial { .. }
        ));
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
