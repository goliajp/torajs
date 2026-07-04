//! `any`-receiver Array method glue (Any-method-call RFC 20260704 C1)
//! — `a.push(v)` / `a.pop()` where `a` crossed into the `any` world.
//!
//! Called by `__torajs_any_method_call`'s Tag::Arr arm
//! (torajs-anyvalue) with already-unboxed receiver pointers; each
//! fn adapts one AnyValue-shaped argument surface onto the existing
//! typed/Any array kernels:
//!
//! - push → kind-matched raw store via the S3-set coercion rules
//!   (`index_any.rs` shapes) + the pool-aware growing
//!   [`crate::grow::__torajs_arr_push`] / Arr<Any>'s
//!   [`crate::any::__torajs_arr_push_any`]. Growth may relocate the
//!   block — the new pointer writes back through `recv_slot` (the
//!   caller's variable slot; NaN-box cell encoding is the raw
//!   pointer bits) when one exists.
//! - pop → reuse the kind-aware boxed read
//!   ([`crate::index_any::__torajs_arr_index_get`], balanced +1 for
//!   cells), release the vacated slot's own reference, shrink len.
//!
//! Reference ledger: arguments are BORROWED from the caller
//! (`ssa_lower_any_method_call` rc-decs every argv slot after the
//! call) — push incs what it stores. Returns follow the boxed-value
//! convention (cells +1 owned by the caller).

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};

const ARR_HEAD_OFF: usize = 20;

/// Sentinel for "a pending throw was recorded" — the dispatcher
/// stops a multi-argument push loop instead of writing past the
/// failure.
pub const ANY_METHOD_THREW: u64 = u64::MAX;

unsafe extern "C" {
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `a.push(…args)` — ES-variadic; each argument appends in order
/// through the kind-coercion table. Returns the new length as a raw
/// u64, or [`ANY_METHOD_THREW`] when a kind-mismatch TypeError was
/// recorded mid-loop (earlier arguments stay appended, matching the
/// ES step-by-step Set semantics).
///
/// Growth may relocate the block — the loop chases the fresh
/// pointer and writes it back through `recv_slot` (NaN-box cell
/// encoding is the raw pointer bits) so the caller's variable stays
/// current.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `argv` points at
/// `argc` live AnyValue slots (borrowed — this fn incs what it
/// stores); `recv_slot` is NULL or the receiver variable's live
/// slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_push(
    arr: *mut c_void,
    argv: *const u64,
    argc: i64,
    recv_slot: *mut u64,
) -> u64 {
    unsafe {
        let mut cur = arr as *mut u8;
        for i in 0..argc {
            let av = *argv.add(i as usize);
            let header = &*(cur as *const HeapHeader);
            let tag = __torajs_anyv_unbox_tag(av);
            let value = __torajs_anyv_unbox_value(av);
            if header.flags & FLAG_ARR_ANY != 0 {
                // Arr<Any> native ledger — the (tag, value) pair ABI
                // transfers one rc for tag 4, so inc the borrowed
                // cell first.
                if tag == 4 && value != 0 {
                    __torajs_rc_inc(value as *mut c_void);
                }
                cur =
                    crate::any::__torajs_arr_push_any(cur as *mut c_void, tag as u64, value as u64);
            } else {
                // Typed tier — same kind-coercion table as the
                // S3-set index write (`__torajs_arr_index_set`).
                let kind = header.arr_elem_kind();
                let raw: u64 = match (kind, tag) {
                    (ARR_KIND_I64, 2) => value as u64,
                    (ARR_KIND_I64, 3) => {
                        let d = f64::from_bits(value as u64);
                        if d.fract() != 0.0 || !d.is_finite() {
                            return push_kind_mismatch();
                        }
                        d as i64 as u64
                    }
                    (ARR_KIND_F64, 3) => value as u64,
                    (ARR_KIND_F64, 2) => (value as f64).to_bits(),
                    (ARR_KIND_BOOL, 1) => value as u64,
                    (ARR_KIND_HEAP, 4) => {
                        __torajs_rc_inc(value as *mut c_void);
                        value as u64
                    }
                    _ => return push_kind_mismatch(),
                };
                cur = crate::grow::__torajs_arr_push(cur, raw as i64);
            }
            if !recv_slot.is_null() {
                *recv_slot = cur as u64;
            }
        }
        *(cur.add(ARR_LEN_OFF) as *const u64)
    }
}

unsafe fn push_kind_mismatch() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"push through an any array receiver would change the array's element kind".as_ptr(),
        );
    }
    ANY_METHOD_THREW
}

/// `a.pop()` — boxed last element (cells +1, ownership to the
/// caller), `undefined` for an empty array. Shrinks len and releases
/// the vacated slot's own reference.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_pop(arr: *mut c_void) -> u64 {
    unsafe {
        let p = arr as *mut u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        let idx = (len - 1) as i64;
        // Kind-aware boxed read — balanced (+1 for cells).
        let out = crate::index_any::__torajs_arr_index_get(arr, idx);
        // Release the vacated slot's own reference (the read above
        // took a fresh one for the caller).
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY != 0 {
            // Arr<Any>: 8-byte NaN-box slots, no deque head (mirrors
            // `any::slot_anyvalue_ptr` / `drop::__torajs_arr_drop_any`).
            let slot = *(p.add(ARR_SLOTS_OFF + ((len - 1) as usize) * 8) as *const u64);
            __torajs_value_drop_heap(slot as *mut c_void);
        } else if header.arr_elem_kind() == ARR_KIND_HEAP {
            let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
            let slot = *(p.add(ARR_SLOTS_OFF + ((head + len - 1) as usize) * 8) as *const u64);
            __torajs_value_drop_heap(slot as *mut c_void);
        }
        *(p.add(ARR_LEN_OFF) as *mut u64) = len - 1;
        out
    }
}
