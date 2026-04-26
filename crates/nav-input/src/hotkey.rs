//! Primary global hotkey: **Alt+** `;` (`VK_OEM_1`) with `MOD_ALT | MOD_NOREPEAT`.
//!
//! Registration uses `RegisterHotKey` on the message-only window (see `thread.rs` on Windows).

/// `RegisterHotKey` id for the primary chord (`Agent/workflow/09-input-handling.md`).
pub const PRIMARY_HOTKEY_ID: i32 = 1;

/// Window class name for the `HWND_MESSAGE` sink (must match `CreateWindow` / `FindWindow`).
#[allow(dead_code)] // Documented contract; wide literal lives in `thread.rs` (`w!(...)` must match).
pub const MESSAGE_WINDOW_CLASS: &str = "Navigator.InputSink.M2";
