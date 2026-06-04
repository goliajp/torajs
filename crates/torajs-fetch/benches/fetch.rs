//! Criterion bench placeholder for `torajs-fetch`. Real HTTP fetch
//! has variable latency dominated by network round-trip + libcurl
//! setup; bench harness is currently a no-op holder so cargo can
//! resolve the [[bench]] entry. Real fetch latency is measured by
//! the integrated bench corpus's `fetch` case (when added).

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("fetch_placeholder", |b| {
        b.iter(|| {
            // Intentionally empty — real fetch is not unit-testable
            // without network state; integration tests cover it.
            42i64
        });
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
