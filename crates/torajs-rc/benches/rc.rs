//! Criterion micro-benches for `torajs-rc`. Mirrors the dominant
//! call shapes from `ssa_lower`-emitted code:
//!
//!  - `inc_dec_pair_ffi` — the FFI path that `ssa_lower` emits
//!    via `call __torajs_rc_inc` / `__torajs_rc_dec`. The
//!    single hottest line in the runtime.
//!  - `inc_dec_pair_method` — the idiomatic Rust `&mut self`
//!    method path that future Rust sub-crates (`torajs-arr`,
//!    `torajs-dynobj`, ...) will use directly without going
//!    through the FFI shim. Should match the FFI path under
//!    fat LTO (the shim is just null-check + reborrow).
//!  - `inc_null_passthrough` — null fast path.
//!  - `inc_static_literal_bypass` — STATIC_LITERAL flag bypass
//!    (hot in tight loops referencing string literals).
//!
//! Run with `cargo bench -p torajs-rc`.

use std::ffi::c_void;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use torajs_rc::{__torajs_rc_dec, __torajs_rc_inc, FLAG_STATIC_LITERAL, HeapHeader, Tag};

// bench binary needs the WeakRef hook; runtime_weakref.c would
// provide it in the real binary. No-op stub matches the runtime
// behavior when no WeakRef is alive (single untaken branch).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}

fn bench_inc_dec_pair_ffi(c: &mut Criterion) {
    let mut h = HeapHeader::new(Tag::Obj);
    let p = &mut h as *mut HeapHeader as *mut c_void;

    c.bench_function("inc_dec_pair_ffi", |b| {
        b.iter(|| {
            unsafe { __torajs_rc_inc(black_box(p)) };
            black_box(unsafe { __torajs_rc_dec(p) });
        });
    });
}

fn bench_inc_dec_pair_method(c: &mut Criterion) {
    let mut h = HeapHeader::new(Tag::Obj);

    c.bench_function("inc_dec_pair_method", |b| {
        b.iter(|| {
            black_box(&mut h).inc_ref();
            black_box(black_box(&mut h).dec_ref());
        });
    });
}

fn bench_inc_null(c: &mut Criterion) {
    c.bench_function("inc_null_passthrough", |b| {
        b.iter(|| {
            unsafe { __torajs_rc_inc(black_box(std::ptr::null_mut())) };
        });
    });
}

fn bench_inc_static_literal(c: &mut Criterion) {
    let mut h = HeapHeader::new(Tag::Str);
    h.flags |= FLAG_STATIC_LITERAL;
    let p = &mut h as *mut HeapHeader as *mut c_void;

    c.bench_function("inc_static_literal_bypass", |b| {
        b.iter(|| {
            unsafe { __torajs_rc_inc(black_box(p)) };
        });
    });
}

criterion_group!(
    benches,
    bench_inc_dec_pair_ffi,
    bench_inc_dec_pair_method,
    bench_inc_null,
    bench_inc_static_literal
);
criterion_main!(benches);
