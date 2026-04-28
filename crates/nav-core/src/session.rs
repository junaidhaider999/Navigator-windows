//! Hint-session state: ingest planned hints, apply keystrokes, emit render / invoke / done events.
//!
//! Models the post-enumeration states from `01-architecture.md` (`Visible`, `Filtered`, invoke path).
//! Idle / enumerating / invoking are owned by `nav-app`; this type resets to finished after `Invoke` or cancel.

use crate::filter::{FilterResult, filter};
use crate::hint::{Hint, HintId};

/// Session state after a hotkey triggers planning. See `03-modules.md`.
///
/// # Example
///
/// ```
/// use nav_core::{Session, SessionEvent, Hint, RawHint, Rect, ElementKind, Backend};
/// let h = Hint {
///     raw: RawHint {
///         element_id: 1,
///         uia_runtime_id_fp: None,
///         uia_invoke_hwnd: None,
///         uia_child_index: None,
///         bounds: Rect { x: 0, y: 0, w: 1, h: 1 },
///         anchor_px: None,
///         kind: ElementKind::Invoke,
///         name: None,
///         backend: Backend::Uia,
///     },
///     label: "a".into(),
///     score: 0.0,
/// };
/// let mut s = Session::new(0);
/// s.ingest(vec![h]);
/// match s.key('a') {
///     SessionEvent::Invoke(id) => assert_eq!(id.0, 0),
///     _ => panic!("expected invoke"),
/// }
/// ```
pub struct Session {
    seed: u64,
    hints: Vec<Hint>,
    prefix: String,
    finished: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionEvent {
    Render(Vec<HintId>),
    Invoke(HintId),
    Done,
}

impl Session {
    /// Creates an empty session. `seed` is reserved for deterministic tie-breaks in later milestones.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            hints: Vec::new(),
            prefix: String::new(),
            finished: false,
        }
    }

    /// Replaces the working hint set (e.g. after `plan`). Clears the typed prefix.
    pub fn ingest(&mut self, hints: Vec<Hint>) {
        self.hints = hints;
        self.prefix.clear();
        self.finished = self.hints.is_empty();
    }

    /// Applies one keystroke while hints are active. `Backspace` / `Delete` pops one UTF-8 char.
    ///
    /// After the first [`SessionEvent::Invoke`] or terminal [`SessionEvent::Done`], further keys yield `Done`.
    pub fn key(&mut self, c: char) -> SessionEvent {
        let _ = self.seed;
        if self.finished {
            return SessionEvent::Done;
        }
        if self.hints.is_empty() {
            self.finished = true;
            return SessionEvent::Done;
        }

        if c == '\u{8}' || c == '\u{7f}' {
            self.prefix.pop();
            return self.after_prefix_change();
        }

        let mut buf = [0u8; 4];
        self.prefix.push_str(c.encode_utf8(&mut buf));

        self.after_prefix_change()
    }

    /// User cancelled (Esc orchestration); ends the session.
    pub fn cancel(&mut self) -> SessionEvent {
        self.finished = true;
        SessionEvent::Done
    }

    /// Current hint list (post-[`ingest`](Self::ingest)).
    #[must_use]
    pub fn hints(&self) -> &[Hint] {
        &self.hints
    }

    /// Prefix typed so far (after backspace normalization via [`key`](Self::key)).
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Hints that should be drawn for the overlay given the current prefix and filter state.
    #[must_use]
    pub fn visible_hints(&self) -> Vec<Hint> {
        if self.finished || self.hints.is_empty() {
            return Vec::new();
        }
        match filter(&self.hints, &self.prefix) {
            FilterResult::None => Vec::new(),
            FilterResult::Single(h) => vec![h.clone()],
            FilterResult::Many(v) => v.iter().map(|h| (*h).clone()).collect(),
        }
    }

    fn after_prefix_change(&mut self) -> SessionEvent {
        match filter(&self.hints, &self.prefix) {
            FilterResult::None => {
                self.finished = true;
                SessionEvent::Done
            }
            FilterResult::Single(h) => {
                let id = self.id_of(h);
                self.finished = true;
                SessionEvent::Invoke(id)
            }
            FilterResult::Many(refs) => {
                let out: Vec<HintId> = refs.into_iter().map(|h| self.id_of(h)).collect();
                SessionEvent::Render(out)
            }
        }
    }

    fn id_of(&self, h: &Hint) -> HintId {
        let ptr = h as *const Hint;
        for (i, x) in self.hints.iter().enumerate() {
            if std::ptr::eq(x as *const Hint, ptr) {
                return HintId(i as u32);
            }
        }
        debug_assert!(false, "hint not in session list");
        HintId(0)
    }
}
