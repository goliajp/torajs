use criterion::{Criterion, criterion_group, criterion_main};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("collections_placeholder", |b| b.iter(|| 42i64));
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
