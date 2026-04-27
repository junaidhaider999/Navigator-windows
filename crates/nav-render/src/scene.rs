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

/// Em height (DIPs) for hint labels; must match `CreateTextFormat` in `d2d::D2dCompositionRenderer::new`.
pub const PILL_FONT_EM_DIPS: f32 = 15.0;
const PILL_PAD_X: f32 = 8.0;
const PILL_PAD_Y: f32 = 5.0;
/// Nudge pill slightly outside the element top-left anchor.
const PILL_OUTSET: f32 = 2.0;
/// Rough average Latin glyph width at [`PILL_FONT_EM_DIPS`] (layout estimate, not shaped text).
const PILL_CHAR_W_EST: f32 = 7.4;
const PILL_MIN_W: f32 = 22.0;
const PILL_MIN_H: f32 = 20.0;
const PILL_MAX_W: f32 = 280.0;

fn estimate_label_width_dips(label: &str) -> f32 {
    let n = label.chars().count().max(1) as f32;
    (n * PILL_CHAR_W_EST).max(10.0)
}

fn pill_size_for_label(label: &str) -> (f32, f32) {
    let tw = estimate_label_width_dips(label);
    let pw = (PILL_PAD_X.mul_add(2.0, tw)).clamp(PILL_MIN_W, PILL_MAX_W);
    let ph = (PILL_PAD_Y.mul_add(2.0, PILL_FONT_EM_DIPS * 1.15)).max(PILL_MIN_H);
    (pw, ph)
}

fn rects_overlap(a: &D2D_RECT_F, b: &D2D_RECT_F) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

fn clamp_pill_to_client(mut r: D2D_RECT_F, client_w: f32, client_h: f32) -> Option<D2D_RECT_F> {
    const MIN_SPAN: f32 = 4.0;
    r.left = r.left.max(0.0);
    r.top = r.top.max(0.0);
    r.right = r.right.min(client_w);
    r.bottom = r.bottom.min(client_h);
    if r.right - r.left >= MIN_SPAN && r.bottom - r.top >= MIN_SPAN {
        Some(r)
    } else {
        None
    }
}

/// TL → TR → BL → BR for each micro-offset ring (§07-style de-collision).
fn pill_rect_candidates(
    el_left: f32,
    el_top: f32,
    el_w: f32,
    el_h: f32,
    pw: f32,
    ph: f32,
) -> Vec<D2D_RECT_F> {
    let o = PILL_OUTSET;
    let corners = [
        (el_left - o, el_top - o),
        (el_left + el_w - pw + o, el_top - o),
        (el_left - o, el_top + el_h - ph + o),
        (el_left + el_w - pw + o, el_top + el_h - ph + o),
    ];
    let mut offs = Vec::with_capacity(40);
    offs.push((0.0f32, 0.0f32));
    for k in 1_i32..=8 {
        let s = k as f32 * 5.0;
        offs.push((s, 0.0));
        offs.push((-s, 0.0));
        offs.push((0.0, s));
        offs.push((0.0, -s));
        offs.push((s * 0.75, s * 0.75));
        offs.push((-s * 0.75, s * 0.75));
    }
    let mut out = Vec::with_capacity(corners.len() * offs.len());
    for (dx, dy) in offs {
        for (l, t) in corners {
            let left = l + dx;
            let top = t + dy;
            out.push(D2D_RECT_F {
                left,
                top,
                right: left + pw,
                bottom: top + ph,
            });
        }
    }
    out
}

fn choose_pill_rect(
    el_left: f32,
    el_top: f32,
    el_w: f32,
    el_h: f32,
    pw: f32,
    ph: f32,
    placed: &[PillGeom],
    client_w: f32,
    client_h: f32,
) -> Option<D2D_RECT_F> {
    for cand in pill_rect_candidates(el_left, el_top, el_w, el_h, pw, ph) {
        if let Some(r) = clamp_pill_to_client(cand, client_w, client_h) {
            if placed.iter().all(|p| !rects_overlap(&r, &p.rect)) {
                return Some(r);
            }
        }
    }
    let o = PILL_OUTSET;
    let l = el_left - o;
    let t = el_top - o;
    clamp_pill_to_client(
        D2D_RECT_F {
            left: l,
            top: t,
            right: l + pw,
            bottom: t + ph,
        },
        client_w,
        client_h,
    )
}

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
    let mut placed = Vec::with_capacity(hints.len());
    for h in hints {
        let el_left = (h.raw.bounds.x - ox) as f32 * scale;
        let el_top = (h.raw.bounds.y - oy) as f32 * scale;
        let el_w = h.raw.bounds.w as f32 * scale;
        let el_h = h.raw.bounds.h as f32 * scale;
        let (pw, ph) = pill_size_for_label(h.label.as_ref());
        if let Some(rect) = choose_pill_rect(
            el_left, el_top, el_w, el_h, pw, ph, &placed, client_w, client_h,
        ) {
            placed.push(PillGeom {
                rect,
                label: h.label.to_string(),
            });
        }
    }
    placed
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
    use nav_core::{Backend, ElementKind, RawHint, Rect};

    fn hint_at(label: &str, element_id: u64, x: i32, y: i32, w: i32, h: i32) -> Hint {
        Hint {
            raw: RawHint {
                element_id,
                uia_invoke_hwnd: None,
                uia_child_index: None,
                bounds: Rect { x, y, w, h },
                kind: ElementKind::Invoke,
                name: None,
                backend: Backend::Uia,
            },
            label: label.into(),
            score: 0.0,
        }
    }

    fn any_pairwise_overlap(pills: &[PillGeom]) -> bool {
        for i in 0..pills.len() {
            for j in (i + 1)..pills.len() {
                if rects_overlap(&pills[i].rect, &pills[j].rect) {
                    return true;
                }
            }
        }
        false
    }

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

    #[test]
    fn adjacent_toolbar_buttons_avoid_overlap() {
        let hints = vec![
            hint_at("a", 1, 0, 0, 24, 24),
            hint_at("s", 2, 24, 0, 24, 24),
            hint_at("d", 3, 48, 0, 24, 24),
        ];
        let pills = pills_for_frame(&hints, (0, 0), 800.0, 200.0, 96.0);
        assert_eq!(pills.len(), 3);
        assert!(
            !any_pairwise_overlap(&pills),
            "expected de-collision, got {:?}",
            pills
        );
    }

    #[test]
    fn longer_label_increases_pill_width() {
        let short = pills_for_frame(&[hint_at("a", 1, 0, 0, 40, 40)], (0, 0), 800.0, 600.0, 96.0);
        let long = pills_for_frame(
            &[hint_at("abc", 1, 0, 0, 40, 40)],
            (0, 0),
            800.0,
            600.0,
            96.0,
        );
        assert_eq!(short.len(), 1);
        assert_eq!(long.len(), 1);
        let sw = short[0].rect.right - short[0].rect.left;
        let lw = long[0].rect.right - long[0].rect.left;
        assert!(
            lw > sw + 4.0,
            "long label should widen pill: short={sw} long={lw}"
        );
    }
}
