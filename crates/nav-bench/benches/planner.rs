use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nav_core::{Backend, ElementKind, RawHint, Rect, UiaEnumerateBasis, plan};

fn sample_raws(n: usize) -> Vec<RawHint> {
    (0..n)
        .map(|i| RawHint {
            element_id: i as u64,
            uia_runtime_id_fp: None,
            uia_invoke_hwnd: None,
            uia_child_index: None,
            uia_enumerate_basis: UiaEnumerateBasis::default(),
            bounds: Rect {
                x: (i as i32 % 40) * 20,
                y: (i as i32 / 40) * 16,
                w: 80,
                h: 24,
            },
            anchor_px: None,
            kind: ElementKind::Invoke,
            name: None,
            backend: Backend::Uia,
        })
        .collect()
}

fn bench_plan(c: &mut Criterion) {
    let alphabet: Vec<char> = "sadfjklewcmpgh".chars().collect();
    let layout_origin = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };
    let mut group = c.benchmark_group("plan");
    for &n in &[14usize, 100, 1024] {
        let raws = sample_raws(n);
        group.bench_function(format!("n_{n}"), |b| {
            b.iter(|| {
                plan(
                    black_box(raws.clone()),
                    black_box(alphabet.as_slice()),
                    black_box(layout_origin),
                    0,
                )
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench_plan);
criterion_main!(benches);
