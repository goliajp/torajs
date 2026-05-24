//! Criterion benches for `torajs-str` on the workspace's hot shapes:
//! - small-Str pool churn (alloc/free cycle of ≤16-byte payload)
//! - byte equality on Str pairs (Array.includes / Map key compare)
//! - slice 64-byte input (CSV / JSON parsing fast path)

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_str::{__torajs_str_eq, __torajs_str_free, __torajs_str_slice, StrBlock};

fn make_str(payload: &[u8]) -> *mut u8 {
    let mut b = StrBlock::alloc(payload.len() as u64);
    let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
    dst.copy_from_slice(payload);
    b.into_raw()
}

fn bench_alloc_free_cycle(c: &mut Criterion) {
    c.bench_function("str_alloc_free_8byte-100k", |b| {
        b.iter(|| {
            for _ in 0..100_000 {
                let p = make_str(black_box(b"abcdefgh"));
                unsafe { __torajs_str_free(p) };
            }
        });
    });
}

fn bench_eq_short(c: &mut Criterion) {
    let a = make_str(b"hello-world-from-torajs-str-bench-corpus-aaaaaaa");
    let b = make_str(b"hello-world-from-torajs-str-bench-corpus-aaaaaaa");
    c.bench_function("str_eq_48byte-100k", |bench| {
        bench.iter(|| {
            let mut acc = 0i64;
            for _ in 0..100_000 {
                acc = acc.wrapping_add(unsafe { __torajs_str_eq(black_box(a), black_box(b)) });
            }
            acc
        });
    });
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
    }
}

fn bench_slice_64byte(c: &mut Criterion) {
    let s = make_str(b"the-quick-brown-fox-jumps-over-the-lazy-dog-aaaaaaaaaaaaaaaaa");
    c.bench_function("str_slice_64byte-100k", |b| {
        b.iter(|| {
            for _ in 0..100_000 {
                let r = unsafe { __torajs_str_slice(black_box(s), 10, 40) };
                unsafe { __torajs_str_free(r) };
            }
        });
    });
    unsafe { __torajs_str_free(s) };
}

criterion_group!(
    benches,
    bench_alloc_free_cycle,
    bench_eq_short,
    bench_slice_64byte
);
criterion_main!(benches);
