//! `Promise.all` over an `Array<Any>` input (mixed promise /
//! plain-value elements) — the any-lane sibling of
//! [`crate::combinator`]'s typed fast path.
//!
//! §27.2.4.1 PerformPromiseAll treats a non-thenable element as an
//! already-fulfilled value (`Promise.resolve(x)` wrap); the typed
//! tier never sees that shape (checker admits homogeneous
//! `Array<Promise<T>>` only), but `Promise.all([Promise.resolve(1),
//! 2])` infers `Array<Any>` whose slots are NaN-box bits — the
//! typed kernel's raw-pointer slot walk would dereference an int32
//! immediate. The entry kernel gates on `FLAG_ARR_ANY` and routes
//! here.
//!
//! Element classification per slot:
//! - NaN-box heap cell with `TAG_PROMISE` → a promise element:
//!   settle state / repr / value read off the cell (mint sites
//!   stamped the repr when the promise crossed into `any`).
//! - everything else (immediates, non-promise cells) → an
//!   already-fulfilled value, stored verbatim.
//!
//! Result: an `Array<Any>` (NaN-box slots, self-describing) inside
//! a `REPR_HEAP` result promise. Same MVP posture as the typed
//! path: a PENDING element (or an UNSTAMPED promise element — a
//! mint family without repr wiring) rejects the outer with the
//! placeholder reason instead of mis-boxing.

use core::ffi::c_void;

use crate::layout::{
    ARR_DATA_PTR_OFF, ARR_HEAD_OFF, ARR_LEN_OFF, Promise, REPR_ANY, REPR_BOOL, REPR_F64, REPR_HEAP,
    REPR_I64, REPR_NULL, REPR_STR, REPR_UNSTAMPED, REPR_VOID, STATE_FULFILLED, STATE_PENDING,
    STATE_REJECTED, TAG_PROMISE,
};

/// `FLAG_ARR_ANY` mirror (torajs-rc `flags.rs`, lockstep).
pub(crate) const FLAG_ARR_ANY: u16 = 1 << 3;

unsafe extern "C" {
    /// torajs-arr — `new Array(n)` any-shape alloc (len=cap=n,
    /// undefined-filled); the build loop overwrites every slot.
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    /// torajs-anyvalue — NaN-box pack / Str-slot boxer / NaN-box-
    /// aware share (immediates no-op).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_box_str_slot(s: *mut c_void) -> u64;
    fn __torajs_anyv_rc_inc(v: u64);
}

/// True when the input array carries NaN-box slots.
pub(crate) unsafe fn arr_is_any(arr: *mut c_void) -> bool {
    unsafe { (*((arr as *mut u8).add(6) as *const u16)) & FLAG_ARR_ANY != 0 }
}

/// Raw NaN-box bits of logical slot `i` (8B stride behind the data
/// pointer — the combinator `arr_slot_ptr` walk, bits not pointer).
unsafe fn any_slot(arr: *mut c_void, i: u64) -> u64 {
    unsafe {
        let bytes = arr as *mut u8;
        let head = *(bytes.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(bytes.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        *(data.add(((head + i) * 8) as usize) as *const u64)
    }
}

/// NaN-box cell gate (mirror of `torajs-anyvalue::nanbox::is_cell`)
/// + promise tag probe. `None` = not a promise element.
unsafe fn slot_promise(bits: u64) -> Option<*mut Promise> {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    if bits == 0 || bits & TOP_16_MASK != 0 || bits & TAG_BIT_TYPE_OTHER != 0 {
        return None;
    }
    let p = bits as *mut u8;
    if unsafe { *(p.add(4) as *const u16) } != TAG_PROMISE {
        return None;
    }
    Some(p as *mut Promise)
}

/// Box a settled promise slot per its repr stamp, OWNED protocol —
/// the result array keeps its own stake (`anyv_rc_inc` is NaN-box
/// aware, immediates pass through). `None` = UNSTAMPED (caller
/// rejects with the MVP placeholder instead of mis-boxing).
unsafe fn box_settled_owned(repr: u8, value: i64) -> Option<u64> {
    let boxed = unsafe {
        match repr {
            REPR_I64 => __torajs_anyv_box_from_pair(2, value),
            REPR_F64 => __torajs_anyv_box_from_pair(3, value),
            REPR_BOOL => __torajs_anyv_box_from_pair(1, value),
            REPR_STR => __torajs_anyv_box_str_slot(value as *mut c_void),
            REPR_HEAP => __torajs_anyv_box_from_pair(4, value),
            REPR_ANY => value as u64,
            REPR_VOID => __torajs_anyv_box_from_pair(5, 0),
            REPR_NULL => __torajs_anyv_box_from_pair(0, 0),
            REPR_UNSTAMPED => return None,
            _ => return None,
        }
    };
    unsafe { __torajs_anyv_rc_inc(boxed) };
    Some(boxed)
}

/// The `Array<Any>` input walk — see module doc. Pre-scan settles
/// the rejection/pending verdict first (spec order: first rejection
/// wins), then the build loop boxes every fulfilled value into the
/// result `Array<Any>`.
pub(crate) unsafe fn all_sync_any(promises_arr: *mut c_void) -> *mut c_void {
    unsafe {
        let len = *((promises_arr as *mut u8).add(ARR_LEN_OFF) as *const u64);
        // Absorb + pre-scan in one walk (the typed path's
        // `absorb_inputs` shape — every promise element counts as
        // handled per the per-element handler attach the spec does).
        for i in 0..len {
            let Some(pp) = slot_promise(any_slot(promises_arr, i)) else {
                continue;
            };
            (*pp).has_handler = 1;
            match (*pp).state {
                STATE_REJECTED => {
                    let Some(reason) = box_settled_owned((*pp).value_repr, (*pp).value) else {
                        return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                    };
                    return crate::combinator::defer_settle(
                        STATE_REJECTED,
                        reason as i64,
                        1,
                        REPR_ANY,
                    );
                }
                STATE_PENDING => {
                    // MVP sync fast path — no fan-in yet (the typed
                    // path's placeholder posture).
                    return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                }
                // Fulfilled — gate UNSTAMPED here so the build loop
                // below never aborts mid-array (which would strand
                // the half-boxed result).
                _ if (*pp).value_repr == REPR_UNSTAMPED => {
                    return crate::combinator::defer_settle(STATE_REJECTED, 0, 0, REPR_VOID);
                }
                _ => {}
            }
        }
        // All fulfilled — build the NaN-box result array.
        let out = __torajs_arr_alloc_any_filled(len);
        let head = *(out.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(out.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for i in 0..len {
            let bits = any_slot(promises_arr, i);
            let v = match slot_promise(bits) {
                // The pre-scan gated UNSTAMPED — the None arm is
                // unreachable; undefined keeps it total without a
                // runtime panic path.
                Some(pp) => box_settled_owned((*pp).value_repr, (*pp).value)
                    .unwrap_or_else(|| __torajs_anyv_box_from_pair(5, 0)),
                None => {
                    // Plain value — stored verbatim, one more owner.
                    __torajs_anyv_rc_inc(bits);
                    bits
                }
            };
            *(data.add(((head + i) * 8) as usize) as *mut u64) = v;
        }
        crate::combinator::defer_settle(STATE_FULFILLED, out as i64, 1, REPR_HEAP)
    }
}
