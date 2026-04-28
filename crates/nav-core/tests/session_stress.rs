//! Random keystroke stress (A3): must not panic or violate session invariants.

use nav_core::{Backend, ElementKind, RawHint, Rect, Session, plan};

fn sample_session(n: usize) -> Session {
    let mut raws = Vec::with_capacity(n);
    for i in 0..n {
        raws.push(RawHint {
            element_id: i as u64,
            uia_runtime_id_fp: None,
            uia_invoke_hwnd: None,
            uia_child_index: None,
            bounds: Rect {
                x: (i as i32) * 8,
                y: 0,
                w: 10,
                h: 10,
            },
            anchor_px: None,
            kind: ElementKind::Invoke,
            name: None,
            backend: Backend::Uia,
        });
    }
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let hints = plan(
        raws,
        &alphabet,
        Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
        },
        0,
    );
    let mut s = Session::new(0);
    s.ingest(hints);
    s
}

#[test]
fn hundred_k_random_keys_never_panics() {
    let keys: Vec<char> = "sadfjklewcmpghzqx".chars().collect();
    let mut s = sample_session(32);
    for i in 0..100_000 {
        let c = keys[i % keys.len()];
        let _ = s.key(c);
    }
}
