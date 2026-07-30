//! Array-subclass instance allocation (RFC
//! 20260730-exotic-backed-class-instance blade 1).
//!
//! `class C extends Array` mints a REAL `Tag::Arr` cell — length
//! magic, index storage, `Array.isArray`, and `instanceof Array` all
//! come for free because the instance IS an array. The class identity
//! rides blade 0's substrate: `FLAG_SUBCLASSED` on the header plus a
//! `cell → (class_tag, proto_cell)` entry in torajs-meta's side
//! table, scrubbed by the drop paths.
//!
//! The cell is always Any-kind: subclass instances live in the `any`
//! world (fields, method receivers, heterogeneous elements), and
//! `new C(n)`'s `super(n)` runs `new Array(n)` semantics (§23.1.2.1)
//! — including the RangeError on a bad length, which the underlying
//! filled alloc already raises (pending-throw protocol: NULL comes
//! back and the caller's throw-check diverts).

use core::ffi::c_void;

use crate::alloc::__torajs_arr_alloc_any_filled;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on `proto_cell`.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
}

/// `torajs_rc::FLAG_SUBCLASSED` (flags bit 0) — imported via the
/// crate's existing dep rather than mirrored; see blade 0.
use torajs_rc::FLAG_SUBCLASSED;

/// Mint an Array-subclass instance: `new Array(len)` semantics, then
/// mark + register the class identity. Returns NULL with a pending
/// RangeError for an out-of-range `len` (the filled alloc raises it).
///
/// # Safety
/// `proto_cell` is the class's process-lifetime `__proto_<C>`
/// singleton; `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_subclass_alloc(
    class_tag: i64,
    proto_cell: u64,
    len: u64,
) -> *mut u8 {
    let p = unsafe { __torajs_arr_alloc_any_filled(len) };
    if p.is_null() {
        return p;
    }
    unsafe {
        *(p.add(6) as *mut u16) |= FLAG_SUBCLASSED;
        __torajs_subclass_register(p as *mut c_void, class_tag, proto_cell);
    }
    p
}
