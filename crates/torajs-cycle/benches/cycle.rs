//! Placeholder — cycle collector benches need cross-tier setup
//! (heap objects with embedded refs) that conformance gate covers
//! end-to-end.

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("cycle_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
