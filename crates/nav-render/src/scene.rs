//! Layout and drawing for overlay “hint pills” (C3: real [`nav_core::Hint`] bounds → client DIPs).
//! Frame classification is NoOp vs full redraw only: partial clips are unsound under
//! `DXGI_SWAP_EFFECT_FLIP_DISCARD` (back-buffer contents undefined after `Present`).

#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, HashSet};

use nav_core::{Hint, UiaDebugReject};
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ROUNDED_RECT, ID2D1Brush, ID2D1DeviceContext,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, IDWriteFactory,
    IDWriteTextFormat,
};
use windows::core::Interface;

use crate::RenderError;

/// One rounded pill with a UTF-8 label.
#[derive(Clone, Debug, PartialEq)]
pub struct PillGeom {
    pub rect: D2D_RECT_F,
    pub label: String,
    /// Multiplier for pill fill/border/text brush opacity (`1.0` = full strength).
    pub opacity: f32,
    /// When connector drawing is enabled: pill center → backing element bbox
    pub debug_connector: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
}

/// Semi-transparent debug rectangle (rejected UIA candidate) in client DIPs.
#[derive(Clone, Debug, PartialEq)]
pub struct DebugRegionGeom {
    pub rect: D2D_RECT_F,
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

fn points_equal(a: D2D_POINT_2F, b: D2D_POINT_2F) -> bool {
    (a.x - b.x).abs() <= f32::EPSILON && (a.y - b.y).abs() <= f32::EPSILON
}

fn connector_equal(
    a: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
    b: Option<(D2D_POINT_2F, D2D_POINT_2F)>,
) -> bool {
    match (a, b) {
        (Some((pa0, pa1)), Some((pb0, pb1))) => points_equal(pa0, pb0) && points_equal(pa1, pb1),
        (None, None) => true,
        _ => false,
    }
}

fn opacity_equal(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// Same set of (label, rect) pairs as last frame (order-independent); used to skip identical paints.
pub fn pills_geometrically_equal(a: &[PillGeom], b: &[PillGeom]) -> bool {
    if duplicate_label(a) || duplicate_label(b) {
        return false;
    }
    if a.len() != b.len() {
        return false;
    }
    type V = (D2D_RECT_F, f32, Option<(D2D_POINT_2F, D2D_POINT_2F)>);
    let mut ma: HashMap<&str, V> = HashMap::with_capacity(a.len());
    for p in a {
        ma.insert(p.label.as_str(), (p.rect, p.opacity, p.debug_connector));
    }
    let mut mb: HashMap<&str, V> = HashMap::with_capacity(b.len());
    for p in b {
        mb.insert(p.label.as_str(), (p.rect, p.opacity, p.debug_connector));
    }
    if ma.len() != mb.len() {
        return false;
    }
    for (k, (r_a, o_a, c_a)) in &ma {
        let Some((r_b, o_b, c_b)) = mb.get(k) else {
            return false;
        };
        if !(rects_equal(*r_a, *r_b) && opacity_equal(*o_a, *o_b) && connector_equal(*c_a, *c_b)) {
            return false;
        }
    }
    true
}

fn debug_regions_equal(a: &[DebugRegionGeom], b: &[DebugRegionGeom]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        if !rects_equal(x.rect, y.rect) {
            return false;
        }
    }
    true
}

/// Decide no-op vs full redraw when pills and optional debug regions are unchanged.
///
/// For pill-only frames, pass empty slices for `old_debug` / `new_debug`.
#[must_use]
pub fn overlay_paint_plan(
    old_pills: &[PillGeom],
    new_pills: &[PillGeom],
    old_debug: &[DebugRegionGeom],
    new_debug: &[DebugRegionGeom],
    client_w: f32,
    client_h: f32,
) -> PaintPlan {
    let _ = (client_w, client_h);
    if pills_geometrically_equal(old_pills, new_pills) && debug_regions_equal(old_debug, new_debug)
    {
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

fn rect_intersection_area(a: &D2D_RECT_F, b: &D2D_RECT_F) -> f32 {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    let right = a.right.min(b.right);
    let bottom = a.bottom.min(b.bottom);
    if left < right && top < bottom {
        (right - left) * (bottom - top)
    } else {
        0.0
    }
}

fn overlap_penalty_with_placed(r: &D2D_RECT_F, placed: &[PillGeom]) -> f32 {
    placed
        .iter()
        .map(|p| rect_intersection_area(r, &p.rect))
        .sum()
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

/// Radial / axis offsets around center + corners; denser rings near the element, wider search when crowded.
fn pill_rect_candidates(
    el_left: f32,
    el_top: f32,
    el_w: f32,
    el_h: f32,
    pw: f32,
    ph: f32,
) -> Vec<D2D_RECT_F> {
    let o = PILL_OUTSET;
    // Prefer center-anchored pills (closer to the clickable hotspot than a corner).
    let cx = el_left + el_w * 0.5 - pw * 0.5;
    let cy = el_top + el_h * 0.5 - ph * 0.5;
    let corners = [
        (cx, cy),
        (el_left - o, el_top - o),
        (el_left + el_w - pw + o, el_top - o),
        (el_left - o, el_top + el_h - ph + o),
        (el_left + el_w - pw + o, el_top + el_h - ph + o),
    ];
    // More rings + slightly tighter step so dense UIA lists still find a nearby free slot.
    const RINGS: i32 = 16;
    const STEP: f32 = 4.0;
    let mut offs = Vec::with_capacity((RINGS as usize).saturating_mul(8).saturating_add(1));
    offs.push((0.0f32, 0.0f32));
    for k in 1_i32..=RINGS {
        let s = k as f32 * STEP;
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
    let cands = pill_rect_candidates(el_left, el_top, el_w, el_h, pw, ph);
    let mut best_overlap: Option<(f32, D2D_RECT_F)> = None;
    for cand in cands {
        if let Some(r) = clamp_pill_to_client(cand, client_w, client_h) {
            if placed.iter().all(|p| !rects_overlap(&r, &p.rect)) {
                return Some(r);
            }
            let pen = overlap_penalty_with_placed(&r, placed);
            best_overlap = match best_overlap {
                None => Some((pen, r)),
                Some((best_p, _)) if pen < best_p => Some((pen, r)),
                Some(prev) => Some(prev),
            };
        }
    }
    if let Some((_, r)) = best_overlap {
        return Some(r);
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
/// Maps rejected UIA rows to client-space rectangles (clipped to the overlay client).
pub fn debug_regions_for_frame(
    rejects: &[UiaDebugReject],
    overlay_origin_phys: (i32, i32),
    client_w: f32,
    client_h: f32,
    dpi: f32,
) -> Vec<DebugRegionGeom> {
    let (ox, oy) = overlay_origin_phys;
    let scale = 96.0 / dpi;
    let mut out = Vec::new();
    for r in rejects {
        let Some(b) = r.bounds else {
            continue;
        };
        if b.w <= 0 || b.h <= 0 {
            continue;
        }
        let left = (b.x - ox) as f32 * scale;
        let top = (b.y - oy) as f32 * scale;
        let right = left + b.w as f32 * scale;
        let bottom = top + b.h as f32 * scale;
        let rect = D2D_RECT_F {
            left,
            top,
            right,
            bottom,
        };
        if let Some(clamped) = clamp_pill_to_client(rect, client_w, client_h) {
            out.push(DebugRegionGeom { rect: clamped });
        }
    }
    out
}

/// With several hints visible, soften low planner scores so dense layouts stay readable.
fn pill_opacity_from_cluster(score: f32, max_score: f32, n_hints: usize) -> f32 {
    const MIN_HINTS: usize = 3;
    if n_hints < MIN_HINTS {
        return 1.0;
    }
    if max_score <= 1e-8 || !max_score.is_finite() {
        return 1.0;
    }
    let t = (score / max_score).clamp(0.0, 1.0);
    0.48 + 0.52 * t
}

pub fn pills_for_frame(
    hints: &[Hint],
    overlay_origin_phys: (i32, i32),
    client_w: f32,
    client_h: f32,
    dpi: f32,
    debug_connectors: bool,
) -> Vec<PillGeom> {
    if hints.is_empty() {
        return Vec::new();
    }
    let (ox, oy) = overlay_origin_phys;
    let scale = 96.0 / dpi;
    let max_score = hints
        .iter()
        .map(|h| h.score)
        .fold(f32::NEG_INFINITY, f32::max);
    let max_score = if max_score.is_finite() {
        max_score
    } else {
        0.0
    };
    let mut order: Vec<usize> = (0..hints.len()).collect();
    order.sort_by(|&a, &b| {
        hints[b]
            .score
            .partial_cmp(&hints[a].score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });
    let mut placed = Vec::with_capacity(hints.len());
    for i in order {
        let h = &hints[i];
        let el_left = (h.raw.bounds.x - ox) as f32 * scale;
        let el_top = (h.raw.bounds.y - oy) as f32 * scale;
        let el_w = h.raw.bounds.w as f32 * scale;
        let el_h = h.raw.bounds.h as f32 * scale;
        let (pw, ph) = pill_size_for_label(h.label.as_ref());
        if let Some(rect) = choose_pill_rect(
            el_left, el_top, el_w, el_h, pw, ph, &placed, client_w, client_h,
        ) {
            let opacity = pill_opacity_from_cluster(h.score, max_score, hints.len());
            let el_cx = el_left + el_w * 0.5;
            let el_cy = el_top + el_h * 0.5;
            let debug_connector = if debug_connectors {
                let px = (rect.left + rect.right) * 0.5;
                let py = (rect.top + rect.bottom) * 0.5;
                Some((
                    D2D_POINT_2F { x: px, y: py },
                    D2D_POINT_2F { x: el_cx, y: el_cy },
                ))
            } else {
                None
            };
            placed.push(PillGeom {
                rect,
                label: h.label.to_string(),
                opacity,
                debug_connector,
            });
        }
    }
    placed
}

const CORNER_RADIUS: f32 = 8.0;

#[inline]
unsafe fn solid_brush_set_opacity(
    br: &ID2D1SolidColorBrush,
    opacity: f32,
) -> Result<(), RenderError> {
    let b: ID2D1Brush = br.cast().map_err(|e| RenderError::Win32(e.to_string()))?;
    b.SetOpacity(opacity);
    Ok(())
}

/// Thin lines from pill center to backing element center (diagnostics). Draw before pills.
pub unsafe fn draw_pill_connectors(
    dc: &ID2D1DeviceContext,
    pills: &[PillGeom],
    stroke: &ID2D1StrokeStyle,
    brush: &ID2D1SolidColorBrush,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    let line_br: ID2D1Brush = brush
        .cast()
        .map_err(|e| RenderError::Win32(e.to_string()))?;
    for pill in pills {
        let Some((from, to)) = pill.debug_connector else {
            continue;
        };
        dc.DrawLine(from, to, &line_br, 1.35, stroke);
    }
    Ok(())
}

/// Fills rounded pills and draws centered labels. Call inside `BeginDraw`/`EndDraw`.
/// Fills translucent debug rectangles (drawn under pills).
pub unsafe fn draw_debug_regions(
    dc: &ID2D1DeviceContext,
    regions: &[DebugRegionGeom],
    fill: &ID2D1SolidColorBrush,
) -> Result<(), RenderError> {
    dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    for r in regions {
        dc.FillRectangle(&r.rect, fill);
    }
    Ok(())
}

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
        solid_brush_set_opacity(fill, pill.opacity)?;
        solid_brush_set_opacity(border, pill.opacity)?;
        solid_brush_set_opacity(text_brush, pill.opacity)?;

        let draw_once = || -> Result<(), RenderError> {
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
            Ok(())
        };

        let r = draw_once();
        let _ = solid_brush_set_opacity(fill, 1.0);
        let _ = solid_brush_set_opacity(border, 1.0);
        let _ = solid_brush_set_opacity(text_brush, 1.0);
        r?;
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

pub fn pill_connector_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.25,
        g: 0.88,
        b: 0.92,
        a: 0.62,
    }
}

pub fn debug_region_fill_color() -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.95,
        g: 0.35,
        b: 0.1,
        a: 0.32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_core::{Backend, ElementKind, RawHint, Rect};

    fn hint_at(label: &str, element_id: u64, x: i32, y: i32, w: i32, h: i32) -> Hint {
        hint_scored(label, element_id, 0.0, x, y, w, h)
    }

    fn hint_scored(
        label: &str,
        element_id: u64,
        score: f32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Hint {
        Hint {
            raw: RawHint {
                element_id,
                uia_runtime_id_fp: None,
                uia_invoke_hwnd: None,
                uia_child_index: None,
                bounds: Rect { x, y, w, h },
                kind: ElementKind::Invoke,
                name: None,
                backend: Backend::Uia,
            },
            label: label.into(),
            score,
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
            opacity: 1.0,
            debug_connector: None,
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
        assert_eq!(
            overlay_paint_plan(&p, &p, &[], &[], 100.0, 100.0),
            PaintPlan::NoOp
        );
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
        assert_eq!(
            overlay_paint_plan(&old, &new, &[], &[], 400.0, 300.0),
            PaintPlan::Full
        );
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
        assert_eq!(
            overlay_paint_plan(&old, &new, &[], &[], 100.0, 100.0),
            PaintPlan::Full
        );
    }

    #[test]
    fn adjacent_toolbar_buttons_avoid_overlap() {
        let hints = vec![
            hint_at("a", 1, 0, 0, 24, 24),
            hint_at("s", 2, 24, 0, 24, 24),
            hint_at("d", 3, 48, 0, 24, 24),
        ];
        let pills = pills_for_frame(&hints, (0, 0), 800.0, 200.0, 96.0, false);
        assert_eq!(pills.len(), 3);
        assert!(
            !any_pairwise_overlap(&pills),
            "expected de-collision, got {:?}",
            pills
        );
    }

    #[test]
    fn higher_score_places_first_closer_to_element_center() {
        let x = 100;
        let y = 100;
        let w = 80;
        let h = 80;
        let low_first = vec![
            hint_scored("low", 1, 0.1, x, y, w, h),
            hint_scored("high", 2, 10.0, x, y, w, h),
        ];
        let pills = pills_for_frame(&low_first, (0, 0), 800.0, 600.0, 96.0, false);
        assert_eq!(pills.len(), 2);
        let cx = (x + w / 2) as f32;
        let cy = (y + h / 2) as f32;
        let dist = |p: &PillGeom| {
            let px = (p.rect.left + p.rect.right) * 0.5;
            let py = (p.rect.top + p.rect.bottom) * 0.5;
            ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
        };
        let d_high = pills.iter().find(|p| p.label == "high").map(dist).unwrap();
        let d_low = pills.iter().find(|p| p.label == "low").map(dist).unwrap();
        assert!(
            d_high <= d_low + 2.0,
            "expected higher-scored hint nearer center: high={d_high} low={d_low} pills={pills:?}"
        );
    }

    #[test]
    fn cluster_scores_dim_lower_priority_pills() {
        let hints = vec![
            hint_scored("a", 1, 10.0, 0, 0, 28, 28),
            hint_scored("b", 2, 6.0, 30, 0, 28, 28),
            hint_scored("c", 3, 2.0, 60, 0, 28, 28),
            hint_scored("d", 4, 0.5, 90, 0, 28, 28),
        ];
        let pills = pills_for_frame(&hints, (0, 0), 800.0, 200.0, 96.0, false);
        let d = pills.iter().find(|p| p.label == "d").expect("d");
        assert!(
            d.opacity < 0.95,
            "expected dimmed low score in cluster: {}",
            d.opacity
        );
        let a = pills.iter().find(|p| p.label == "a").expect("a");
        assert!(a.opacity > d.opacity, "a={} d={}", a.opacity, d.opacity);
    }

    #[test]
    fn debug_connectors_when_requested() {
        let hints = vec![hint_at("x", 1, 50, 50, 40, 40)];
        let with_c = pills_for_frame(&hints, (0, 0), 800.0, 600.0, 96.0, true);
        assert!(
            with_c[0].debug_connector.is_some(),
            "{:?}",
            with_c[0].debug_connector
        );
        let without = pills_for_frame(&hints, (0, 0), 800.0, 600.0, 96.0, false);
        assert!(without[0].debug_connector.is_none());
    }

    #[test]
    fn longer_label_increases_pill_width() {
        let short = pills_for_frame(
            &[hint_at("a", 1, 0, 0, 40, 40)],
            (0, 0),
            800.0,
            600.0,
            96.0,
            false,
        );
        let long = pills_for_frame(
            &[hint_at("abc", 1, 0, 0, 40, 40)],
            (0, 0),
            800.0,
            600.0,
            96.0,
            false,
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
