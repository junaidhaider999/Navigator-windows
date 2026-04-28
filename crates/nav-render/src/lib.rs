//! Layered full-screen overlay (Windows). Phase C2: Direct2D + DirectComposition on a DXGI swap
//! chain bound to the overlay HWND.

#![cfg_attr(windows, allow(unsafe_op_in_unsafe_fn))]

mod error;
pub use error::RenderError;

#[cfg(windows)]
mod d2d;
#[cfg(windows)]
mod monitors;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod scene;

#[cfg(windows)]
use crossbeam_channel::{Sender, unbounded};
#[cfg(windows)]
use nav_core::{Hint, UiaDebugReject};

#[cfg(windows)]
use std::thread::JoinHandle;

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayRenderOpts {
    /// Pill center → invoke anchor connector lines.
    pub debug_connectors: bool,
    /// Red dot at resolved invoke anchor (physical→DIP mapped).
    pub debug_target_dot: bool,
    /// Green outline around element bounds.
    pub debug_target_rect: bool,
    /// Numeric distance (pill center to anchor, DIPs).
    pub debug_distance: bool,
}

/// Owns the render worker thread and sends [`overlay::RenderCmd`] commands.
#[cfg(windows)]
pub struct Renderer {
    cmd: Sender<overlay::RenderCmd>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl Renderer {
    /// Spawns the overlay thread (message pump + layered window).
    pub fn spawn() -> Result<Self, RenderError> {
        let (tx, rx) = unbounded();
        overlay::set_render_command_sender(tx.clone());
        let thread = std::thread::Builder::new()
            .name("navigator-render".into())
            .spawn(move || overlay::run_render_thread(rx))
            .map_err(|e| RenderError::Win32(e.to_string()))?;
        Ok(Self {
            cmd: tx,
            thread: Some(thread),
        })
    }

    /// Broadcast monitor/DPI sync (e.g. after config reload). Overlay HWNDs also react to `WM_DISPLAYCHANGE` / `WM_DPICHANGED`.
    pub fn sync_monitors(&self) -> Result<(), RenderError> {
        self.cmd
            .send(overlay::RenderCmd::SyncMonitors)
            .map_err(|_| RenderError::Disconnected)
    }

    /// Warms the overlay HWND and GPU stack while hidden (D2). Safe to call once after [`spawn`](Self::spawn).
    pub fn prewarm(&self) -> Result<(), RenderError> {
        self.cmd
            .send(overlay::RenderCmd::Prewarm)
            .map_err(|_| RenderError::Disconnected)
    }

    /// Shows the overlay for `session_id`. `hints` are copied to the worker; C2 draws the demo
    /// pill strip (see `04-build-order.md`); C3 will use real bounds.
    pub fn show(
        &self,
        session_id: u64,
        hints: &[Hint],
        debug_rejects: &[UiaDebugReject],
        opts: OverlayRenderOpts,
    ) -> Result<(), RenderError> {
        self.cmd
            .send(overlay::RenderCmd::Show {
                session_id,
                hints: hints.to_vec(),
                debug_rejects: debug_rejects.to_vec(),
                opts,
            })
            .map_err(|_| RenderError::Disconnected)
    }

    pub fn hide(&self, session_id: u64) -> Result<(), RenderError> {
        self.cmd
            .send(overlay::RenderCmd::Hide { session_id })
            .map_err(|_| RenderError::Disconnected)
    }

    /// Updates pill geometry for an already-shown `session_id` (does not change `max_show_accepted`).
    pub fn repaint(
        &self,
        session_id: u64,
        hints: &[Hint],
        debug_rejects: &[UiaDebugReject],
        opts: OverlayRenderOpts,
    ) -> Result<(), RenderError> {
        self.cmd
            .send(overlay::RenderCmd::Repaint {
                session_id,
                hints: hints.to_vec(),
                debug_rejects: debug_rejects.to_vec(),
                opts,
            })
            .map_err(|_| RenderError::Disconnected)
    }

    /// Stops the worker and joins. Prefer this over relying on [`Drop`] for deterministic teardown
    /// in tests.
    pub fn shutdown(mut self) -> Result<(), RenderError> {
        let _ = self.cmd.send(overlay::RenderCmd::Shutdown);
        if let Some(t) = self.thread.take() {
            t.join().map_err(|_| RenderError::ThreadExited)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.cmd.send(overlay::RenderCmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Stub on non-Windows so `cargo check --workspace` succeeds in CI.
#[cfg(not(windows))]
pub struct Renderer {
    _private: (),
}

#[cfg(not(windows))]
impl Renderer {
    pub fn spawn() -> Result<Self, RenderError> {
        Err(RenderError::UnsupportedPlatform)
    }

    pub fn prewarm(&self) -> Result<(), RenderError> {
        Err(RenderError::UnsupportedPlatform)
    }

    pub fn sync_monitors(&self) -> Result<(), RenderError> {
        Err(RenderError::UnsupportedPlatform)
    }
}
