//! Date-subclass instance allocation (rotation 373 — extends RFC
//! 20260730-exotic-backed-class-instance blade 2 to Date, the
//! torajs-collections / torajs-weak `subclass_alloc` twin).
//!
//! `class C extends Date` mints a REAL Date cell whose [[DateValue]]
//! is the current wall clock — the no-argument `new Date()` answer —
//! so a bare `super()` is a no-op against the mint. The
//! one-argument `super(v)` (§21.4.2.1 step 4) overwrites the mint's
//! ms via `__torajs_date_set_ms_from` (the torajs-anyvalue super
//! kernel resolves `v` with the full ToPrimitive/parse ladder and
//! hands back a scratch Date). The whole getter/setter/format
//! surface rides the existing arms because the instance IS a Date.
//! Class identity rides blade 0 (`FLAG_SUBCLASSED` + torajs-meta
//! side table), scrubbed by `__torajs_date_drop`.

use core::ffi::c_void;

use crate::Date;
use crate::api::__torajs_date_now;

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0 — same mirror the collections twin carries).
pub(crate) const FLAG_SUBCLASSED: u16 = 1;

/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// Mint a Date-subclass instance ([[DateValue]] = current wall
/// clock) and answer it boxed — subclass instances live in the any
/// world.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let p = __torajs_date_now();
        (*(p as *mut Date)).header.flags |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, p as i64)
    }
}

/// Copy `src`'s [[DateValue]] into `dst` — the invalid-date sentinel
/// rides verbatim, so a refused coercion lands as Invalid Date
/// exactly like the plain one-argument ctor.
///
/// # Safety
/// Both pointers are live Date cells.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_ms_from(dst: *mut c_void, src: *const c_void) {
    if dst.is_null() || src.is_null() {
        return;
    }
    unsafe {
        (*(dst as *mut Date)).ms = (*(src as *const Date)).ms;
    }
}
