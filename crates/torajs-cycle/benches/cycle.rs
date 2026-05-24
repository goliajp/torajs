//! Placeholder — cycle collector benches need cross-tier setup
//! (heap objects with embedded refs) that conformance gate covers
//! end-to-end.

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("cycle_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
