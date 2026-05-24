//! Criterion micro-benches for `torajs-anyvalue` hot paths.
//!
//!  - `box_unbox_i64` — alloc + read tag + read value + drop. The
//!    end-to-end shape every `Type::Any` callsite hits.
//!  - `box_heap_rc_paths` — Heap-tagged alloc / drop pair, which
//!    exercises rc_inc + value_drop_heap call (stubbed in this
//!    bench binary).
//!  - `payload_rc_inc_inline` — the no-op fast path for inline
//!    tags. Should compile to one compare + branch.
//!
//! Run with `cargo bench -p torajs-anyvalue`.

use std::ffi::c_void;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use torajs_anyvalue::{AnyBox, payload_rc_inc};
use torajs_rc::{AnySlotTag, HeapHeader, Tag};

// Bench binary needs runtime extern "C" symbols torajs-anyvalue
// and torajs-rc declare. In the shipped binary they come from
// runtime_weakref.c and torajs-rc's libtorajs_rc.a; here they
// no-op (bench binaries DCE the rlib's dispatch fn).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut c_void) {}

fn bench_box_unbox_i64(c: &mut Criterion) {
    c.bench_function("box_unbox_i64", |b| {
        b.iter(|| {
            let p = AnyBox::alloc(AnySlotTag::I64, black_box(42));
            unsafe {
                let t = p.as_ref().tag;
                let v = p.as_ref().value;
                black_box((t, v));
                AnyBox::drop_owned(p);
            }
        });
    });
}

fn bench_box_heap_rc_paths(c: &mut Criterion) {
    let mut child = HeapHeader::new(Tag::Str);
    let child_ptr = &mut child as *mut HeapHeader;

    c.bench_function("box_heap_alloc_drop", |b| {
        b.iter(|| {
            let p = AnyBox::alloc(AnySlotTag::Heap, child_ptr as i64);
            unsafe { AnyBox::drop_owned(p) };
        });
    });
}

fn bench_payload_rc_inc_inline(c: &mut Criterion) {
    c.bench_function("payload_rc_inc_inline_tag", |b| {
        b.iter(|| {
            payload_rc_inc(black_box(2 /* I64 */), black_box(42));
        });
    });
}

criterion_group!(
    benches,
    bench_box_unbox_i64,
    bench_box_heap_rc_paths,
    bench_payload_rc_inc_inline,
);
criterion_main!(benches);
