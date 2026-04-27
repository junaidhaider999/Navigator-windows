use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nav_core::{Backend, ElementKind, Hint, RawHint, Rect, Session};

fn sample_session_hints(n: usize) -> Vec<Hint> {
    let raw = RawHint {
        element_id: 0,
        uia_runtime_id_fp: None,
        uia_invoke_hwnd: None,
        uia_child_index: None,
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
    (0..n)
        .map(|i| Hint {
            raw: RawHint {
                element_id: i as u64,
                ..raw.clone()
            },
            label: format!("a{i:x}").into(),
            score: 0.0,
        })
        .collect()
}

fn bench_session_key(c: &mut Criterion) {
    let hints = sample_session_hints(1024);
    let mut group = c.benchmark_group("session_key");
    group.bench_function("first_key_n1024", |b| {
        b.iter(|| {
            let mut s = Session::new(0);
            s.ingest(black_box(hints.clone()));
            black_box(s.key('x'))
        })
    });
    group.finish();
}

fn bench_session_filter_chain(c: &mut Criterion) {
    let hints = sample_session_hints(256);
    let mut group = c.benchmark_group("session_chain");
    group.bench_function("type_3_chars_n256", |b| {
        b.iter(|| {
            let mut s = Session::new(0);
            s.ingest(black_box(hints.clone()));
            let _ = s.key('a');
            let _ = s.key('b');
            black_box(s.key('c'))
        })
    });
    group.finish();
}

criterion_group!(benches, bench_session_key, bench_session_filter_chain);
criterion_main!(benches);
