//! Criterion micro-benches for `torajs-anyvalue` hot paths.
//!
//!  - `anyv_box_unbox_i64` — NaN-box an i64 + unbox tag + unbox
//!    value. The end-to-end shape every `Type::Any` callsite hits
//!    post Step 7 (no AnyBox heap alloc; immediate u64).
//!  - `anyv_rc_inc_inline` — rc-inc on a primitive immediate
//!    (Int32). Should compile to one predicate + branch (cell path
//!    not taken).
//!  - `payload_rc_inc_inline` — the no-op fast path for inline
//!    tags. Should compile to one compare + branch.
//!
//! Run with `cargo bench -p torajs-anyvalue`.

use std::ffi::c_void;
use std::hint::black_box;

use torajs_anyvalue::{
    __torajs_anyv_box_i64, __torajs_anyv_rc_inc, __torajs_anyv_unbox_tag,
    __torajs_anyv_unbox_value, payload_rc_inc,
};
use torajs_bench::{Bench, bench_group, bench_main};

// Bench binary needs runtime extern "C" symbols torajs-anyvalue
// and torajs-rc declare. In the shipped binary they come from
// runtime_weakref.c and torajs-rc's libtorajs_rc.a; here they
// no-op (bench binaries DCE the rlib's dispatch fn).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut c_void) {}

fn bench_anyv_box_unbox_i64(c: &mut Bench) {
    c.bench_function("anyv_box_unbox_i64", |b| {
        b.iter(|| {
            let v = __torajs_anyv_box_i64(black_box(42));
            let t = __torajs_anyv_unbox_tag(v);
            let val = __torajs_anyv_unbox_value(v);
            black_box((t, val));
        });
    });
}

fn bench_anyv_rc_inc_inline(c: &mut Bench) {
    c.bench_function("anyv_rc_inc_inline", |b| {
        b.iter(|| unsafe {
            __torajs_anyv_rc_inc(black_box(__torajs_anyv_box_i64(42)));
        });
    });
}

fn bench_payload_rc_inc_inline(c: &mut Bench) {
    c.bench_function("payload_rc_inc_inline_tag", |b| {
        b.iter(|| {
            payload_rc_inc(black_box(2 /* I64 */), black_box(42));
        });
    });
}

bench_group!(
    benches,
    bench_anyv_box_unbox_i64,
    bench_anyv_rc_inc_inline,
    bench_payload_rc_inc_inline,
);
bench_main!(benches);
