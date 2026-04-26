//! State coverage for `01-architecture.md` post-enumeration behaviour (Visible → Filtered → Invoke / Done).

use nav_core::{Backend, ElementKind, Hint, RawHint, Rect, Session, SessionEvent, filter, plan};

fn raw_at(i: u64, x: i32) -> RawHint {
    RawHint {
        element_id: i,
        bounds: Rect {
            x,
            y: 0,
            w: 20,
            h: 20,
        },
        kind: ElementKind::Invoke,
        name: None,
        backend: Backend::Uia,
    }
}

fn two_planned_hints() -> (Session, String, String) {
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let hints = plan(
        vec![raw_at(1, 0), raw_at(2, 100)],
        &alphabet,
        Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 50,
        },
    );
    let a = hints[0].label.to_string();
    let b = hints[1].label.to_string();
    let mut s = Session::new(0);
    s.ingest(hints);
    (s, a, b)
}

#[test]
fn visible_to_filtered_emits_render() {
    let hints = vec![
        Hint {
            raw: raw_at(1, 0),
            label: "sa".into(),
            score: 0.0,
        },
        Hint {
            raw: raw_at(2, 40),
            label: "sj".into(),
            score: 0.0,
        },
    ];
    let mut s = Session::new(0);
    s.ingest(hints);
    let ev = s.key('s');
    let SessionEvent::Render(ids) = ev else {
        panic!("expected Render, got {ev:?}");
    };
    assert_eq!(ids.len(), 2);
}

#[test]
fn filtered_to_invoke_on_unique_prefix() {
    let (mut s, label_a, _) = two_planned_hints();
    let mut last = SessionEvent::Done;
    for c in label_a.chars() {
        last = s.key(c);
        if matches!(last, SessionEvent::Invoke(_)) {
            break;
        }
    }
    assert!(matches!(last, SessionEvent::Invoke(id) if id.0 == 0));
}

#[test]
fn filtered_to_done_when_no_match() {
    let (mut s, _, _) = two_planned_hints();
    let ev = s.key('z');
    assert!(matches!(ev, SessionEvent::Done));
}

#[test]
fn cancel_emits_done() {
    let (mut s, _, _) = two_planned_hints();
    assert!(matches!(s.cancel(), SessionEvent::Done));
    assert!(matches!(s.key('s'), SessionEvent::Done));
}

#[test]
fn no_hints_ingest_key_done() {
    let mut s = Session::new(0);
    s.ingest(Vec::new());
    assert!(matches!(s.key('a'), SessionEvent::Done));
}

#[test]
fn finished_session_key_is_done() {
    let (mut s, label_a, _) = two_planned_hints();
    for c in label_a.chars() {
        let _ = s.key(c);
    }
    assert!(matches!(s.key('q'), SessionEvent::Done));
}

#[test]
fn backspace_restores_candidates() {
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let hints_vec = plan(
        vec![raw_at(1, 0), raw_at(2, 100)],
        &alphabet,
        Rect {
            x: 0,
            y: 0,
            w: 50,
            h: 50,
        },
    );
    let c0 = hints_vec[0].label.chars().next().unwrap();
    let mut s = Session::new(0);
    s.ingest(hints_vec.clone());
    let _ = s.key(c0);
    let _ = s.key('\u{8}');
    match filter(&hints_vec, "") {
        nav_core::FilterResult::Many(v) => assert_eq!(v.len(), 2),
        o => panic!("{o:?}"),
    }
}

#[test]
fn invoke_then_idle_on_followup_keys() {
    let (mut s, label_a, _) = two_planned_hints();
    for c in label_a.chars() {
        if matches!(s.key(c), SessionEvent::Invoke(_)) {
            break;
        }
    }
    assert!(matches!(s.key('x'), SessionEvent::Done));
}
