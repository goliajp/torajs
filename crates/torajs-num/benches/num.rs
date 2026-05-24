//! Criterion benches on the workspace's hot-loop number-crunch
//! patterns: dense Math.sqrt / Math.pow loops (mandelbrot,
//! prime_count) and parseInt / parseFloat at csv-like data
//! processing scale.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

use torajs_num::parse::{parse_float, parse_int};

unsafe extern "C" {
    fn __torajs_math_sqrt(x: f64) -> f64;
    fn __torajs_math_pow(x: f64, y: f64) -> f64;
    fn __torajs_math_floor(x: f64) -> f64;
}

fn bench_sqrt_hot_loop(c: &mut Criterion) {
    c.bench_function("math_sqrt-100k", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for i in 0..100_000 {
                acc += unsafe { __torajs_math_sqrt(black_box(i as f64)) };
            }
            acc
        });
    });
}

fn bench_pow_hot_loop(c: &mut Criterion) {
    c.bench_function("math_pow-10k", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for i in 0..10_000 {
                acc += unsafe { __torajs_math_pow(black_box(i as f64 / 100.0), black_box(2.5)) };
            }
            acc
        });
    });
}

fn bench_floor(c: &mut Criterion) {
    c.bench_function("math_floor-100k", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for i in 0..100_000 {
                acc += unsafe { __torajs_math_floor(black_box(i as f64 / 7.0)) };
            }
            acc
        });
    });
}

fn bench_parse_int(c: &mut Criterion) {
    let inputs: Vec<&[u8]> = vec![b"0", b"42", b"-17", b"  -7  ", b"0xff", b"123456", b"+100"];
    c.bench_function("parse_int-mixed-1k", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for _ in 0..1_000 {
                for s in &inputs {
                    acc += parse_int(black_box(s), 10);
                }
            }
            acc
        });
    });
}

fn bench_parse_float(c: &mut Criterion) {
    let inputs: Vec<&[u8]> = vec![b"0.0", b"1.5", b"-3.14159", b"1e10", b"1.5e-3", b"Infinity"];
    c.bench_function("parse_float-mixed-1k", |b| {
        b.iter(|| {
            let mut acc = 0.0f64;
            for _ in 0..1_000 {
                for s in &inputs {
                    acc += parse_float(black_box(s));
                }
            }
            acc
        });
    });
}

criterion_group!(
    benches,
    bench_sqrt_hot_loop,
    bench_pow_hot_loop,
    bench_floor,
    bench_parse_int,
    bench_parse_float
);
criterion_main!(benches);
