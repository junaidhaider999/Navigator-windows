//! Global hotkey registration and the input pump for Navigator.
//!
//! On Windows: a message-only window receives `WM_HOTKEY` and forwards
//! [`InputEvent`] values on a `crossbeam-channel` queue. The `WM_HOTKEY` path
//! only performs timing, `GetForegroundWindow`, and a channel send — no extra
//! heap work in the window procedure beyond what the channel may do internally.

mod hotkey;

#[cfg(windows)]
mod focus;

#[cfg(windows)]
mod chord;

#[cfg(windows)]
mod thread;

#[cfg(windows)]
pub use thread::InputThread;

#[cfg(not(windows))]
mod stub;

#[cfg(not(windows))]
pub use stub::InputThread;

/// Snapshot emitted when the primary hotkey fires (registered chord, e.g. `Alt+;`, or plain `/` when safe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyPress {
    /// `RegisterHotKey` id (`wParam` of `WM_HOTKEY`); same as primary id for plain-`/` activations.
    pub id: i32,
    /// `GetForegroundWindow` at trigger time (pointer value as `usize`).
    pub captured_hwnd: usize,
    /// Time inside the `WM_HOTKEY` handler: ΔQPC from before `GetForegroundWindow` to after, in microseconds.
    pub latency_us: u64,
    /// `true` when this activation used plain `/` (or numpad `/`) because focus was not in a typical text field.
    pub from_plain_slash: bool,
}

/// Keystrokes delivered while hint mode is active (low-level hook; see C3 in `04-build-order.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKey {
    Char(char),
    Escape,
    Backspace,
}

/// Events from the input worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Hotkey(HotkeyPress),
    SessionKey(SessionKey),
}

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("nav-input is only supported on Windows")]
    UnsupportedPlatform,
    #[error("could not register global hotkey: {details}")]
    HotkeyRegisterFailed { details: String },
    #[cfg(windows)]
    #[error(transparent)]
    Win32(#[from] windows::core::Error),
    #[error("input thread exited before reporting hotkey registration status")]
    ThreadEndedDuringStartup,
}

#[cfg(windows)]
/// Second-instance handshake: ask the running Navigator to bring its console forward.
pub fn poke_peer_for_foreground() {
    thread::poke_peer_for_foreground();
}

#[cfg(not(windows))]
pub fn poke_peer_for_foreground() {}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    #[test]
    fn spawn_errors_on_non_windows() {
        assert!(matches!(
            crate::InputThread::spawn_with_chord("alt+;"),
            Err(crate::InputError::UnsupportedPlatform)
        ));
    }
}
