//! Direct2D + DirectComposition swap-chain renderer for the overlay HWND (C2).

use std::time::{Duration, Instant};

use nav_core::Hint;
use tracing::debug;
use windows::Win32::Foundation::{BOOL, HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_SOLID, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_LINE_JOIN_MITER, D2D1_STROKE_STYLE_PROPERTIES1,
    D2D1CreateFactory, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush,
    ID2D1StrokeStyle1,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionTarget,
    IDCompositionVisual2,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_FEATURE_PRESENT_ALLOW_TEARING,
    DXGI_PRESENT, DXGI_PRESENT_ALLOW_TEARING, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGIFactory5, IDXGISurface,
    IDXGISwapChain1,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};
use windows::core::{Interface, w};

use crate::RenderError;
use crate::scene::{self, PillGeom};

const FRAME_BUDGET: Duration = Duration::from_millis(4);

/// GPU-backed overlay: DXGI flip swap chain + DComp target + D2D text and rounded fills.
#[allow(dead_code)] // Fields hold COM refs for correct drop order; not all are read after init.
pub struct D2dCompositionRenderer {
    hwnd: HWND,
    pixel_w: u32,
    pixel_h: u32,
    dpi: f32,
    d3d: ID3D11Device,
    d2d_factory: ID2D1Factory1,
    d2d_device: ID2D1Device,
    d2d_ctx: ID2D1DeviceContext,
    dcomp: IDCompositionDesktopDevice,
    dcomp_target: IDCompositionTarget,
    root_visual: IDCompositionVisual2,
    swap_chain: IDXGISwapChain1,
    write: IDWriteFactory,
    text_format: IDWriteTextFormat,
    stroke: ID2D1StrokeStyle1,
    pill_fill: ID2D1SolidColorBrush,
    pill_border: ID2D1SolidColorBrush,
    pill_text: ID2D1SolidColorBrush,
    /// `Present(0, ALLOW_TEARING)` is only valid when the swap chain was created with
    /// [`DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING`] after a successful factory check.
    present_allow_tearing: bool,
    swap_chain_flags: DXGI_SWAP_CHAIN_FLAG,
    last_pills: Vec<PillGeom>,
}

impl D2dCompositionRenderer {
    pub unsafe fn new(hwnd: HWND) -> Result<Self, RenderError> {
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client)
            .map_err(|e| RenderError::Win32(format!("GetClientRect: {e}")))?;
        let pixel_w = (client.right - client.left).max(1) as u32;
        let pixel_h = (client.bottom - client.top).max(1) as u32;
        let dpi = {
            let d = GetDpiForWindow(hwnd);
            if d > 0 { d as f32 } else { 96.0 }
        };

        let mut device: Option<ID3D11Device> = None;
        let flags = D3D11_CREATE_DEVICE_FLAG(D3D11_CREATE_DEVICE_BGRA_SUPPORT.0);
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .map_err(|e| RenderError::Win32(e.to_string()))?;
        let d3d =
            device.ok_or_else(|| RenderError::Win32("D3D11CreateDevice returned null".into()))?;

        let dxgi_device: IDXGIDevice = d3d.cast().map_err(|e| RenderError::Win32(e.to_string()))?;
        let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let mut present_allow_tearing = false;
        if let Ok(factory5) = dxgi_factory.cast::<IDXGIFactory5>() {
            let mut allow = BOOL(0);
            let hr = factory5.CheckFeatureSupport(
                DXGI_FEATURE_PRESENT_ALLOW_TEARING,
                &mut allow as *mut BOOL as *mut std::ffi::c_void,
                std::mem::size_of::<BOOL>() as u32,
            );
            if hr.is_ok() {
                present_allow_tearing = allow.as_bool();
            }
        }
        let swap_chain_flags = if present_allow_tearing {
            DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING
        } else {
            DXGI_SWAP_CHAIN_FLAG(0)
        };

        let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        let d2d_device = d2d_factory
            .CreateDevice(&dxgi_device)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        let d2d_ctx = d2d_device
            .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let dcomp: IDCompositionDesktopDevice =
            DCompositionCreateDevice2(&d3d).map_err(|e| RenderError::Win32(e.to_string()))?;

        let root_visual = dcomp
            .CreateVisual()
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let swap_chain = {
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: pixel_w,
                Height: pixel_h,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: swap_chain_flags.0 as u32,
            };
            // `CreateSwapChainForHwnd` + layered `WS_POPUP` often returns `DXGI_ERROR_INVALID_CALL`
            // (flip model is tied to the HWND presentation path). Composition swap chains are
            // HWND-agnostic; DComp binds them via `SetContent` + `CreateTargetForHwnd`.
            dxgi_factory
                .CreateSwapChainForComposition(
                    &d3d,
                    &desc,
                    None::<&windows::Win32::Graphics::Dxgi::IDXGIOutput>,
                )
                .map_err(|e| {
                    RenderError::Win32(format!(
                        "CreateSwapChainForComposition ({}×{}, flags={}): {e}",
                        pixel_w, pixel_h, desc.Flags
                    ))
                })?
        };

        root_visual
            .SetContent(&swap_chain)
            .map_err(|e| RenderError::Win32(format!("DComp SetContent(swap_chain): {e}")))?;

        let dcomp_target = dcomp
            .CreateTargetForHwnd(hwnd, true)
            .map_err(|e| RenderError::Win32(format!("DComp CreateTargetForHwnd: {e}")))?;
        dcomp_target
            .SetRoot(&root_visual)
            .map_err(|e| RenderError::Win32(format!("DComp SetRoot: {e}")))?;
        dcomp
            .Commit()
            .map_err(|e| RenderError::Win32(format!("DComp Commit: {e}")))?;

        let write: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        let text_format = write
            .CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                15.0,
                w!("en-us"),
            )
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let stroke_props = D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: D2D1_CAP_STYLE_FLAT,
            endCap: D2D1_CAP_STYLE_FLAT,
            dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_MITER,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
            ..Default::default()
        };
        let stroke = d2d_factory
            .CreateStrokeStyle(&stroke_props, None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        d2d_ctx.SetDpi(dpi, dpi);

        let pill_fill = d2d_ctx
            .CreateSolidColorBrush(&scene::pill_fill_color(), None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        let pill_border = d2d_ctx
            .CreateSolidColorBrush(&scene::pill_border_color(), None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        let pill_text = d2d_ctx
            .CreateSolidColorBrush(&scene::pill_text_color(), None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let origin = window_origin_phys(hwnd)?;
        let last_pills = scene::pills_for_frame(
            &[],
            origin,
            client_w_dips(pixel_w, dpi),
            client_h_dips(pixel_h, dpi),
            dpi,
        );

        Ok(Self {
            hwnd,
            pixel_w,
            pixel_h,
            dpi,
            d3d,
            d2d_factory,
            d2d_device,
            d2d_ctx,
            dcomp,
            dcomp_target,
            root_visual,
            swap_chain,
            write,
            text_format,
            stroke,
            pill_fill,
            pill_border,
            pill_text,
            present_allow_tearing,
            swap_chain_flags,
            last_pills,
        })
    }

    /// Rebuild swap chain buffers if the HWND client size or DPI changed.
    pub unsafe fn sync_size_and_dpi(&mut self) -> Result<(), RenderError> {
        let mut client = RECT::default();
        GetClientRect(self.hwnd, &mut client)
            .map_err(|e| RenderError::Win32(format!("GetClientRect: {e}")))?;
        let nw = (client.right - client.left).max(1) as u32;
        let nh = (client.bottom - client.top).max(1) as u32;
        let dpi = {
            let d = GetDpiForWindow(self.hwnd);
            if d > 0 { d as f32 } else { 96.0 }
        };

        if nw == self.pixel_w && nh == self.pixel_h && (dpi - self.dpi).abs() < 0.01 {
            return Ok(());
        }

        self.pixel_w = nw;
        self.pixel_h = nh;
        self.dpi = dpi;
        self.d2d_ctx.SetDpi(dpi, dpi);

        self.swap_chain
            .ResizeBuffers(2, nw, nh, DXGI_FORMAT_B8G8R8A8_UNORM, self.swap_chain_flags)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let origin = window_origin_phys(self.hwnd)?;
        self.last_pills = scene::pills_for_frame(
            &[],
            origin,
            client_w_dips(nw, dpi),
            client_h_dips(nh, dpi),
            dpi,
        );
        self.dcomp
            .Commit()
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        Ok(())
    }

    /// Rebuild scene from hints and present. Returns wall time spent in D2D + Present + Commit.
    pub unsafe fn update_and_present(&mut self, hints: &[Hint]) -> Result<Duration, RenderError> {
        let t0 = Instant::now();
        self.sync_size_and_dpi()?;

        let cw = client_w_dips(self.pixel_w, self.dpi);
        let ch = client_h_dips(self.pixel_h, self.dpi);
        let origin = window_origin_phys(self.hwnd)?;
        let new_pills = scene::pills_for_frame(hints, origin, cw, ch, self.dpi);
        if matches!(
            scene::paint_plan(&self.last_pills, &new_pills, cw, ch),
            scene::PaintPlan::NoOp
        ) {
            self.last_pills = new_pills;
            return Ok(t0.elapsed());
        }

        let surface: IDXGISurface = self
            .swap_chain
            .GetBuffer(0)
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let px_format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let bmp_props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: px_format,
            dpiX: self.dpi,
            dpiY: self.dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };

        let target = self
            .d2d_ctx
            .CreateBitmapFromDxgiSurface(&surface, Some(&bmp_props))
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        self.d2d_ctx.SetTarget(&target);
        self.d2d_ctx.BeginDraw();
        let clear = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };

        self.d2d_ctx.Clear(Some(&clear));
        scene::draw_pills(
            &self.d2d_ctx,
            &self.text_format,
            &self.write,
            &new_pills,
            &self.pill_fill,
            &self.pill_border,
            &self.pill_text,
            &self.stroke,
        )?;

        self.last_pills = new_pills;

        self.d2d_ctx
            .EndDraw(None, None)
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        drop(target);

        self.present_frame()?;
        self.dcomp
            .Commit()
            .map_err(|e| RenderError::Win32(e.to_string()))?;

        let elapsed = t0.elapsed();
        if elapsed > FRAME_BUDGET {
            debug!(
                target: "nav_render",
                us = elapsed.as_micros(),
                "paint exceeded C2 frame budget (4 ms)",
            );
        }
        Ok(elapsed)
    }

    unsafe fn present_frame(&self) -> Result<(), RenderError> {
        if self.present_allow_tearing {
            self.swap_chain
                .Present(0, DXGI_PRESENT_ALLOW_TEARING)
                .ok()
                .map_err(|e| RenderError::Win32(e.to_string()))?;
        } else {
            // Without tearing, interval 0 + no tearing flag is invalid on flip-model swap chains.
            self.swap_chain
                .Present(1, DXGI_PRESENT(0))
                .ok()
                .map_err(|e| RenderError::Win32(e.to_string()))?;
        }
        Ok(())
    }
}

fn window_origin_phys(hwnd: HWND) -> Result<(i32, i32), RenderError> {
    let mut wr = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut wr) }
        .map_err(|e| RenderError::Win32(format!("GetWindowRect: {e}")))?;
    Ok((wr.left, wr.top))
}

fn client_w_dips(px_w: u32, dpi: f32) -> f32 {
    px_w as f32 * 96.0 / dpi
}

fn client_h_dips(px_h: u32, dpi: f32) -> f32 {
    px_h as f32 * 96.0 / dpi
}
