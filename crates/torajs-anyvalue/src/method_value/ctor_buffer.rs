//! The buffer family's constructor faces — `ArrayBuffer`, the twelve
//! typed arrays and `DataView` constructed through a runtime value,
//! and a typed array's constructor slot read off its element kind.
//! Moved out of [`super::ctor`] (rotation 655, file-size: that table
//! sits past its cap) so the family's slot layout lives in one place.

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    /// torajs-buffer — §25.1.4.1 / §23.2.5.1, reached when one of
    /// the twelve buffer-family constructors is CONSTRUCTED through
    /// a runtime value (RFC 20260823-typedarray-substrate 刀 4).
    fn __torajs_arraybuffer_create(length: u64, options: u64) -> u64;
    fn __torajs_typedarray_create(kind: i64, a0: u64, a1: u64, a2: u64) -> u64;
    fn __torajs_typedarray_kind(av: u64) -> i64;
    /// torajs-buffer — §25.3.2 `new DataView(buffer, byteOffset,
    /// byteLength)`, the same kernel the static `new` lowering calls.
    fn __torajs_dataview_create(buffer: u64, a1: u64, a2: u64) -> u64;
}

/// `DataView.prototype`'s builtin-proto slot — past the eleven
/// per-kind typed-array slots, so it is not `tag - 20` of anything
/// (565-01).
pub(super) const DATAVIEW_TAG: i64 = torajs_rc::builtin_proto::DATAVIEW_PROTO_TAG as i64;

/// [[Construct]] for a buffer-family slot (`tag >= 19`).
///
/// RFC 20260823-typedarray-substrate 刀 4 — the twelve buffer
/// families share one shape: read the fixed argument slots and hand
/// them to the kernel that the static `new` lowering calls with the
/// same values.
///
/// # Safety
/// `argv` points at `argc` readable AnyValue slots.
pub(super) unsafe fn construct(tag: i64, argv: *const u64, argc: i64) -> u64 {
    let at = |i: i64| -> u64 {
        if i < argc {
            unsafe { argv.add(i as usize).read() }
        } else {
            VALUE_UNDEFINED
        }
    };
    if tag == 19 {
        return unsafe { __torajs_arraybuffer_create(at(0), at(1)) };
    }
    // 565-01 — DataView's slot sits past the per-kind block, so it is
    // NOT `tag - 20` of anything: `new dv(buf)` through a
    // `const dv: any = DataView` binding was minting a Float16Array
    // off kind 12.
    if tag == DATAVIEW_TAG {
        return unsafe { __torajs_dataview_create(at(0), at(1), at(2)) };
    }
    unsafe { __torajs_typedarray_create(tag - 20, at(0), at(1), at(2)) }
}

/// A typed array's constructor slot — decided by its element kind,
/// since the eleven share one heap tag (RFC 20260823-typedarray-
/// substrate 刀 4).
///
/// # Safety
/// `recv` is a live `Tag::TypedArray` cell.
pub(super) unsafe fn typedarray_ctor_tag(recv: AnyValue) -> Option<i64> {
    let kind = unsafe { __torajs_typedarray_kind(recv) };
    (0..=11).contains(&kind).then_some(20 + kind)
}
