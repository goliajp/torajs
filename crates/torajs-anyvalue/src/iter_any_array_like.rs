//! §23.1.2.1 step 3's OTHER half — the array-like walk `Array.from`
//! falls back to when the source turns out not to be iterable.
//!
//! It belongs to `Array.from` alone. `[...x]` on the same `{length:
//! 3}` object must throw, so this lane is reached only through
//! `__torajs_any_iter_next_array_like`; the plain
//! `__torajs_any_iter_next` still ends the cascade with the TypeError.
//!
//! "Not iterable" is the verdict of the WHOLE cascade in
//! [`crate::iter_any`], not just "no user `@@iterator`" — a string, an
//! Array, a Map and a class instance all lack a user method yet
//! iterate. So this lane sits exactly where the throw used to be, and
//! only the entry that opted in ever reaches it.
//!
//! `length` is read ONCE per loop (§23.1.2.1 step 3.b LengthOfArrayLike
//! runs before the walk, so a `length` accessor observes one call, not
//! one per element). `iter_slot` is free to cache it: this lane is
//! past every lane that parks something there, and the caller releases
//! the slot as an AnyValue, which an immediate integer no-ops.

use crate::coerce::any_to_number;
use crate::index_any::__torajs_any_index_get;
use crate::member_get::__torajs_any_member_get_tag;
use crate::member_get_value::__torajs_any_member_get_value;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_int32, box_int32, is_int32};
use core::sync::atomic::{AtomicPtr, Ordering};

unsafe extern "C" {
    fn __torajs_str_alloc_ascii(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_rc_dec(p: *mut core::ffi::c_void) -> i32;
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
}

/// The interned `"length"` key cell. Minted once and never released —
/// immortal by construction, and every consumer only READS it (the
/// same contract as `iter_any_get_method`'s step-tier names).
static NAME_LENGTH: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

/// # Safety
/// Called only from the iteration lane below, on the runtime's own
/// allocator.
unsafe fn length_key() -> *mut u8 {
    let existing = NAME_LENGTH.load(Ordering::Acquire);
    if !existing.is_null() {
        return existing;
    }
    let fresh = unsafe { __torajs_str_alloc_ascii(b"length".as_ptr(), 6) };
    match NAME_LENGTH.compare_exchange(
        core::ptr::null_mut(),
        fresh,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => fresh,
        Err(winner) => {
            unsafe { __torajs_rc_dec(fresh as *mut core::ffi::c_void) };
            winner
        }
    }
}

/// §7.1.20 LengthOfArrayLike: ToLength of the receiver's `length`.
/// Clamped to what this walk can actually build — pushing more than
/// `i32::MAX` AnyValues exhausts memory long before the cap matters,
/// and the cap is what lets the cached length ride in an immediate.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn array_like_length(recv: AnyValue) -> i32 {
    unsafe {
        let key = length_key() as *const core::ffi::c_void;
        let tag = __torajs_any_member_get_tag(recv, key);
        let payload = __torajs_any_member_get_value(recv, key);
        let n = any_to_number(tag as i64, payload as i64);
        if !(n > 0.0) {
            // NaN, negative and zero all answer an empty walk.
            return 0;
        }
        // §10.4.2.2 ArrayCreate step 1 — both consumers of this walk
        // (`Array.from` step 3.c / `Array.fromAsync` §3.k.v) feed the
        // length straight into ArrayCreate, which throws RangeError
        // above 2³²−1. Gating here (with a pending throw the caller's
        // step check turns into the throw/reject) instead of clamping
        // — the clamp walked ~2³¹ indexes before answering, which
        // reads as a hang.
        if n > 4294967295.0 {
            __torajs_throw_range_error(c"invalid array length".as_ptr());
            return 0;
        }
        if n >= i32::MAX as f64 {
            return i32::MAX;
        }
        n as i32
    }
}

/// One step of the array-like walk. Answers 1 with `out` holding an
/// OWNED element (the indexed lane's contract — `__torajs_any_index_get`
/// hands back a fresh reference), 0 when the walk is done.
///
/// # Safety
/// `recv` is a live AnyValue; `idx_slot` / `iter_slot` / `out` are
/// valid writable pointers owned by the caller's loop.
pub(crate) unsafe fn step(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        let len = if is_int32(*iter_slot) {
            as_int32(*iter_slot)
        } else {
            let len = array_like_length(recv);
            *iter_slot = box_int32(len);
            len
        };
        let idx = *idx_slot;
        if idx >= len as i64 {
            *out = VALUE_UNDEFINED;
            return 0;
        }
        *idx_slot = idx + 1;
        // Every absent index answers undefined, which is exactly what
        // §23.1.2.1 step 3.e.iv's Get produces for a hole.
        *out = __torajs_any_index_get(recv, idx);
        1
    }
}
