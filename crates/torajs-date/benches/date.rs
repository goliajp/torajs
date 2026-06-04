//! Placeholder — Date construction + getter benches need cross-tier
//! setup (rc / anyvalue) that isn't trivially available in a standalone
//! cargo bench. Integration coverage via conformance gate fixtures.

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("date_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
