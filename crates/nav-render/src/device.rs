//! GDI resources for `UpdateLayeredWindow` (C1 baseline — replaced by D2D/DComp in C2).

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};

use crate::RenderError;

fn dib_stride_bytes(width: i32) -> usize {
    (width as usize).saturating_mul(32).div_ceil(32).saturating_mul(4)
}

/// Fills a top-down 32-bpp DIB: fully transparent, plus an opaque **100×100** physical-pixel
/// marker at the top-left (DPI sanity check per `04-build-order.md` C1).
pub unsafe fn fill_bgra_layer(bits: *mut u8, stride: usize, area: RECT) -> Result<(), RenderError> {
    if bits.is_null() {
        return Err(RenderError::Win32(
            "CreateDIBSection returned null bits".into(),
        ));
    }
    let w = (area.right - area.left).max(0) as usize;
    let h = (area.bottom - area.top).max(0) as usize;
    if w == 0 || h == 0 {
        return Ok(());
    }
    let sz = stride
        .checked_mul(h)
        .ok_or_else(|| RenderError::Win32("bitmap size overflow".into()))?;
    std::ptr::write_bytes(bits, 0, sz);
    let mark = 100usize;
    for yy in 0..mark.min(h) {
        for xx in 0..mark.min(w) {
            let idx = yy
                .checked_mul(stride)
                .and_then(|o| o.checked_add(xx.saturating_mul(4)))
                .ok_or_else(|| RenderError::Win32("pixel offset overflow".into()))?;
            *bits.add(idx) = 0;
            *bits.add(idx + 1) = 255;
            *bits.add(idx + 2) = 255;
            *bits.add(idx + 3) = 255;
        }
    }
    Ok(())
}

/// Creates a memory DC + 32-bpp top-down DIB covering `area` (physical pixels).
pub unsafe fn create_layer_dc(area: RECT) -> Result<(HDC, HBITMAP, HGDIOBJ), RenderError> {
    let w = area.right - area.left;
    let h = area.bottom - area.top;
    if w <= 0 || h <= 0 {
        return Err(RenderError::Win32("non-positive overlay dimensions".into()));
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp: HBITMAP = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        .map_err(|e| RenderError::Win32(e.to_string()))?;

    let mem_dc = CreateCompatibleDC(None);
    if mem_dc.is_invalid() {
        let _ = DeleteObject(bmp.into());
        return Err(RenderError::Win32("CreateCompatibleDC failed".into()));
    }

    let old = SelectObject(mem_dc, bmp.into());
    if old.is_invalid() {
        let _ = DeleteDC(mem_dc);
        let _ = DeleteObject(bmp.into());
        return Err(RenderError::Win32("SelectObject failed".into()));
    }

    let stride = dib_stride_bytes(w);
    fill_bgra_layer(bits as *mut u8, stride, area)?;

    Ok((mem_dc, bmp, old))
}

/// Deletes GDI objects created by [`create_layer_dc`].
pub unsafe fn destroy_layer_dc(mem_dc: HDC, bmp: HBITMAP, old: HGDIOBJ) {
    if !mem_dc.is_invalid() {
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem_dc);
    }
}
