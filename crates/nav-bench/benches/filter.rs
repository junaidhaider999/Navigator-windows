use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nav_core::{Backend, ElementKind, Hint, RawHint, Rect, filter};

fn sample_hints(n: usize) -> Vec<Hint> {
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
    let alphabet: Vec<char> = "abcdefghij".chars().collect();
    (0..n)
        .map(|i| Hint {
            raw: RawHint {
                element_id: i as u64,
                ..raw.clone()
            },
            label: format!(
                "{}{}",
                alphabet[i % alphabet.len()],
                alphabet[(i / alphabet.len()) % alphabet.len()]
            )
            .into(),
            score: 0.0,
        })
        .collect()
}

fn bench_filter_prefixes(c: &mut Criterion) {
    let hints = sample_hints(1024);
    let mut group = c.benchmark_group("filter");
    for prefix in ["", "a", "ab", "abc"] {
        group.bench_function(format!("prefix_{prefix}_n1024"), |b| {
            b.iter(|| filter(black_box(&hints), black_box(prefix)))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_filter_prefixes);
criterion_main!(benches);
