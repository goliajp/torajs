//! Placeholder — dynobj benches need str + rc setup; conformance
//! gate covers end-to-end timing via the bench corpus (csv-trim
//! etc. use property-bag access).

use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("dynobj_placeholder", |b| {
        b.iter(|| 42i64);
    });
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
