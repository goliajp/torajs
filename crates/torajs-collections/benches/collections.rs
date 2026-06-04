use torajs_bench::{Bench, bench_group, bench_main};

fn bench_placeholder(c: &mut Bench) {
    c.bench_function("collections_placeholder", |b| b.iter(|| 42i64));
}

bench_group!(benches, bench_placeholder);
bench_main!(benches);
