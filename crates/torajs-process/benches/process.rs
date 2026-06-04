//! Criterion placeholder. process.* ops are syscall-dominated;
//! end-to-end coverage via the conformance gate fixtures.

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("process_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
