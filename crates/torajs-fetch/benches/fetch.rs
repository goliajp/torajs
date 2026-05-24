//! Criterion bench placeholder for `torajs-fetch`. Real HTTP fetch
//! has variable latency dominated by network round-trip + libcurl
//! setup; bench harness is currently a no-op holder so cargo can
//! resolve the [[bench]] entry. Real fetch latency is measured by
//! the integrated bench corpus's `fetch` case (when added).

use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("fetch_placeholder", |b| {
        b.iter(|| {
            // Intentionally empty — real fetch is not unit-testable
            // without network state; integration tests cover it.
            42i64
        });
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
