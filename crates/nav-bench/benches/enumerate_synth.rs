//! Synthetic enumeration pipeline: raw list shape similar to a medium UIA tree (no COM).

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nav_core::{
    Backend, ElementKind, RawHint, Rect, UiaEnumerateBasis, dedupe_raw_hints, fnv1a_hash_i32_slice,
    plan,
};

fn synthetic_raws(n: usize) -> Vec<RawHint> {
    (0..n)
        .map(|i| RawHint {
            element_id: i as u64,
            uia_runtime_id_fp: Some(fnv1a_hash_i32_slice(&[
                (i % 4096) as i32,
                (i / 4096) as i32,
            ])),
            uia_invoke_hwnd: None,
            uia_child_index: None,
            uia_enumerate_basis: UiaEnumerateBasis::default(),
            bounds: Rect {
                x: (i % 64) as i32 * 12,
                y: (i / 64) as i32 * 10,
                w: 48,
                h: 20,
            },
            anchor_px: None,
            kind: ElementKind::Invoke,
            name: None,
            backend: Backend::Uia,
        })
        .collect()
}

fn bench_dedupe_then_plan(c: &mut Criterion) {
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let layout = Rect {
        x: 0,
        y: 0,
        w: 1280,
        h: 800,
    };
    let mut group = c.benchmark_group("dedupe_plan");
    for &n in &[256usize, 1024] {
        let raws = synthetic_raws(n);
        group.bench_function(format!("n_{n}"), |b| {
            b.iter_with_setup(
                || raws.clone(),
                |input| {
                    let (deduped, _st) = dedupe_raw_hints(input);
                    black_box(plan(
                        deduped,
                        black_box(alphabet.as_slice()),
                        black_box(layout),
                        0,
                    ))
                },
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_dedupe_then_plan);
criterion_main!(benches);
