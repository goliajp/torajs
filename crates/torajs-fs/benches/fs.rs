//! Criterion bench placeholder for `torajs-fs`. Real fs latency
//! depends on disk cache state + filesystem; not meaningful to
//! microbench inside cargo. End-to-end coverage via the conformance
//! gate's `fs` fixtures.

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("fs_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
