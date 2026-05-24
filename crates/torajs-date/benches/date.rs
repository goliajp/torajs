//! Placeholder — Date construction + getter benches need cross-tier
//! setup (rc / anyvalue) that isn't trivially available in a standalone
//! cargo bench. Integration coverage via conformance gate fixtures.

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("date_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
