//! Criterion micro-benches for `torajs-ucd` lookups.
//!
//! Two suites:
//!  - `is_letter_cp_hit_cjk` — common case: lookup a CJK codepoint
//!    that resides near the tail of the table. Binary search depth ≈
//!    ceil(log2(N)) where N ≈ 50 → ~6 iters.
//!  - `is_letter_cp_miss` — codepoint NOT in any range. Same binary
//!    search depth; tests no-hit path.
//!  - `is_number_cp` — analogous for the smaller Number table.
//!
//! Run with `cargo bench -p torajs-ucd`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use torajs_ucd::{is_letter_cp, is_number_cp};

fn bench_is_letter_cp_hit_cjk(c: &mut Criterion) {
    c.bench_function("is_letter_cp_hit_cjk", |b| {
        b.iter(|| is_letter_cp(black_box(0x4E2D)));
    });
}

fn bench_is_letter_cp_hit_greek(c: &mut Criterion) {
    c.bench_function("is_letter_cp_hit_greek", |b| {
        b.iter(|| is_letter_cp(black_box(0x03B1)));
    });
}

fn bench_is_letter_cp_miss(c: &mut Criterion) {
    c.bench_function("is_letter_cp_miss", |b| {
        b.iter(|| is_letter_cp(black_box(0x2022)));
    });
}

fn bench_is_number_cp_hit_arabic_indic(c: &mut Criterion) {
    c.bench_function("is_number_cp_hit_arabic_indic", |b| {
        b.iter(|| is_number_cp(black_box(0x0665)));
    });
}

fn bench_is_number_cp_miss(c: &mut Criterion) {
    c.bench_function("is_number_cp_miss", |b| {
        b.iter(|| is_number_cp(black_box(0x4E2D)));
    });
}

criterion_group!(
    benches,
    bench_is_letter_cp_hit_cjk,
    bench_is_letter_cp_hit_greek,
    bench_is_letter_cp_miss,
    bench_is_number_cp_hit_arabic_indic,
    bench_is_number_cp_miss,
);
criterion_main!(benches);
