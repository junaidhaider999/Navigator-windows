//! Layout and drawing for overlay “hint pills”. C2: five hard-coded demo hints (`aa`…`ae`).
//! C3 will map [`nav_core::Hint`] bounds into client space; until then the demo strip is used
//! regardless of `hints` (see `04-build-order.md`).

#![allow(clippy::too_many_arguments)]

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
pub struct PillGeom {
    pub rect: D2D_RECT_F,
    pub label: String,
}

const DEMO_LABELS: [&str; 5] = ["aa", "ab", "ac", "ad", "ae"];

/// Builds pill geometry for the current frame (C2 demo strip).
pub fn pills_for_frame(_hints: &[Hint], client_w: f32, client_h: f32) -> Vec<PillGeom> {
    c2_demo_pills(client_w, client_h)
}

fn c2_demo_pills(client_w: f32, client_h: f32) -> Vec<PillGeom> {
    let pill_w = 88.0f32;
    let pill_h = 36.0f32;
    let gap = 12.0f32;
    let total_w = 5.0 * pill_w + 4.0 * gap;
    let start_x = ((client_w - total_w).max(0.0)) * 0.5;
    let y = client_h * 0.18;

    DEMO_LABELS
        .iter()
        .enumerate()
        .map(|(i, lab)| PillGeom {
            rect: D2D_RECT_F {
                left: start_x + i as f32 * (pill_w + gap),
                top: y,
                right: start_x + i as f32 * (pill_w + gap) + pill_w,
                bottom: y + pill_h,
            },
            label: (*lab).to_string(),
        })
        .collect()
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
