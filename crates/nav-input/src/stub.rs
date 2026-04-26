//! Non-Windows stub: API surface compiles; [`InputThread::spawn`](super::InputThread::spawn) errors.

use crossbeam_channel::Receiver;

use crate::{InputError, InputEvent};

pub struct InputThread;

impl InputThread {
    pub fn spawn() -> Result<(Self, Receiver<InputEvent>), InputError> {
        Err(InputError::UnsupportedPlatform)
    }
}
