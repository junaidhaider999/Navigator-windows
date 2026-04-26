//! Allocation gate for `Session::key` steady state (`12-benchmarking.md`).
//!
//! Uses `dhat` in **testing** mode with the crate `Alloc` global allocator. We assert that once a
//! session has invoked a hint, follow-up `key` calls (`Done` path) do not increase the cumulative
//! allocation count.

use dhat::{Alloc, HeapStats, Profiler};

#[global_allocator]
static ALLOC: Alloc = Alloc;

use nav_core::{Backend, ElementKind, RawHint, Rect, Session, SessionEvent, plan};

fn one_hint_session() -> Session {
    let raw = RawHint {
        element_id: 1,
        bounds: Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        },
        kind: ElementKind::Invoke,
        name: None,
        backend: Backend::Uia,
    };
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let hints = plan(
        vec![raw],
        &alphabet,
        Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        },
    );
    let mut session = Session::new(0);
    session.ingest(hints);
    session
}

#[test]
fn session_key_finished_path_no_heap_growth() {
    let _profiler = Profiler::builder().testing().build();

    let mut session = one_hint_session();
    assert!(matches!(session.key('s'), SessionEvent::Invoke(_)));

    let before = HeapStats::get().total_blocks;
    assert!(matches!(session.key('z'), SessionEvent::Done));
    assert!(matches!(session.key('z'), SessionEvent::Done));
    let after = HeapStats::get().total_blocks;

    assert_eq!(
        after, before,
        "Session::key must not allocate once the session is finished"
    );
}
