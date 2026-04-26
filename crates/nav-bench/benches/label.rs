use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nav_core::generate_labels;

fn hap_alphabet() -> Vec<char> {
    "sadfjklewcmpgh".chars().collect()
}

fn bench_generate_labels(c: &mut Criterion) {
    let alphabet = hap_alphabet();
    let mut group = c.benchmark_group("generate_labels");
    for &n in &[14usize, 100, 1024, 5000] {
        group.bench_function(format!("n_{n}"), |b| {
            b.iter(|| generate_labels(black_box(n), black_box(alphabet.as_slice())))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_generate_labels);
criterion_main!(benches);
