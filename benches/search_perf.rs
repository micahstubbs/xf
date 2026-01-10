//! Search performance benchmarks for xf.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn search_benchmark(c: &mut Criterion) {
    // TODO: Add actual benchmarks once we have test data
    c.bench_function("placeholder", |b| {
        b.iter(|| {
            black_box(1 + 1)
        })
    });
}

criterion_group!(benches, search_benchmark);
criterion_main!(benches);
