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
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-anyvalue — NaN-box encode / decode.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `torajs_rc::FLAG_SUBCLASSED` (flags bit 0) — imported via the
/// crate's existing dep rather than mirrored; see blade 0.
use torajs_rc::{__torajs_rc_inc, FLAG_SUBCLASSED};

use crate::any::{__torajs_arr_extend_any, ANY_HEAP, ANY_UNDEF};

/// Mint an Array-subclass instance: `new Array(len)` semantics, then
/// mark + register the class identity (prototype resolved from the
/// classmeta registry — module init registered it before any factory
/// can run). Returns the instance as a boxed AnyValue — subclass
/// instances live in the any world (the factory's `__this` is `any`).
/// A bad `len` raises RangeError (pending-throw protocol) and answers
/// boxed undefined for the throw-check to divert past.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_subclass_alloc(class_tag: i64, len: u64) -> u64 {
    let p = unsafe { __torajs_arr_alloc_any_filled(len) };
    if p.is_null() {
        return unsafe { __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0) };
    }
    unsafe {
        *(p.add(6) as *mut u16) |= FLAG_SUBCLASSED;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p as *mut c_void, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP as i64, p as i64)
    }
}

/// `super(len)` inside an Array-subclass ctor — `new Array(len)`
/// semantics applied to the already-minted instance (§23.1.2.1):
/// validate the length, then grow to `len` undefined-filled slots.
/// The instance left the alloc with len 0 and this-TDZ means nothing
/// was stored before `super()` ran, so appending is exact. Raises
/// RangeError (pending-throw protocol) on a bad length. Takes and
/// returns the instance as a boxed AnyValue (the ctor's `__this` is
/// `any`); the cell survives growth (B1: push reallocs the data
/// buffer, never the cell).
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged); `len_av`
/// is any boxed value (ToNumber runs here, §23.1.2.1 step 4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_subclass_super_len(this_av: u64, len_av: u64) -> u64 {
    let len = unsafe { __torajs_anyv_to_number(len_av) };
    const MAX_ARRAY_LEN: f64 = u32::MAX as f64;
    if !(len >= 0.0 && len <= MAX_ARRAY_LEN && (len as u64) as f64 == len) {
        unsafe {
            __torajs_throw_range_error(
                b"Array length must be a positive integer of safe magnitude.\0".as_ptr(),
            );
        }
        return this_av;
    }
    unsafe {
        let p = __torajs_anyv_unbox_value(this_av) as *mut c_void;
        if p.is_null() {
            return this_av;
        }
        let undef_tag = ANY_UNDEF as u64;
        for _ in 0..len as u64 {
            crate::any::__torajs_arr_push_any(p, undef_tag, 0);
        }
        // The answer is a fresh owned reference: `super(n)` sits in
        // statement position and the lowerer releases the discarded
        // any value — answering the borrowed alias un-inc'd let that
        // release steal the ctor's own reference (the churn probe
        // caught the fallout as a per-instance leak through the
        // refcount underflow).
        __torajs_rc_inc(p);
    }
    this_av
}

unsafe extern "C" {
    fn __torajs_throw_range_error(msg: *const u8);
    /// torajs-anyvalue — ToNumber over a boxed any (§7.1.4).
    fn __torajs_anyv_to_number(v: u64) -> f64;
}

/// `super(a, b, ...)` with 2+ arguments inside an Array-subclass
/// ctor — `new Array(a, b, ...)` elements semantics (§23.1.1.3):
/// every argument appends as an element. The synthesized rest-param
/// default ctor hands its packed rest array (an Arr<Any> cell,
/// boxed); the walk borrows it — `arr_extend_any` incs each shared
/// slot, so both arrays own their references.
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged);
/// `elems_av` is a live borrowed boxed Arr<Any>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_subclass_super_elems(this_av: u64, elems_av: u64) -> u64 {
    unsafe {
        let this_p = __torajs_anyv_unbox_value(this_av) as *mut u8;
        let elems_p = __torajs_anyv_unbox_value(elems_av) as *const u8;
        if this_p.is_null() || elems_p.is_null() {
            return this_av;
        }
        __torajs_arr_extend_any(this_p, elems_p);
        // Fresh owned answer — same account as the len twin above:
        // `super(...)` sits in statement position and the lowerer
        // releases the discarded any value.
        __torajs_rc_inc(this_p as *mut c_void);
    }
    this_av
}
