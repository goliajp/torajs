//! Criterion placeholder for `torajs-meta`. The three concerns
//! (fnprops / classmeta / reflect) need cross-tier setup
//! (registered classes, fn-instance pointers, etc.) which the
//! conformance gate provides end-to-end; standalone microbench
//! would need significant scaffolding to set up a realistic state.

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("meta_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
