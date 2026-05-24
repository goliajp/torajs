//! Placeholder — dynobj benches need str + rc setup; conformance
//! gate covers end-to-end timing via the bench corpus (csv-trim
//! etc. use property-bag access).

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("dynobj_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
