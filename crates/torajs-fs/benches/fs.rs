//! Criterion bench placeholder for `torajs-fs`. Real fs latency
//! depends on disk cache state + filesystem; not meaningful to
//! microbench inside cargo. End-to-end coverage via the conformance
//! gate's `fs` fixtures.

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("fs_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
