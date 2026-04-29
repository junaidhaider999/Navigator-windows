//! Non-Windows stub: API surface compiles; [`InputThread::spawn`](super::InputThread::spawn) errors.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Receiver;

use crate::{InputError, InputEvent};

pub struct InputThread {
    pub hint_mode: Arc<AtomicBool>,
    pub keyboard_passthrough: Arc<AtomicBool>,
}

impl InputThread {
    pub fn spawn_with_chord(_chord: &str) -> Result<(Self, Receiver<InputEvent>), InputError> {
        Err(InputError::UnsupportedPlatform)
    }

    pub fn reregister_hotkey(&self, _chord: &str) -> Result<(), InputError> {
        Err(InputError::UnsupportedPlatform)
    }
}
