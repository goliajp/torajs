//! `__torajs_any_iter_next` — the unified for-of iteration protocol
//! over an `any` receiver (Any-dynamic-access RFC 20260704 S5+).
//!
//! One runtime call per iteration replaces the old two-phase shape
//! (hoisted `__torajs_any_iter_len` + per-iter `recv[i]`), extending
//! `for (x of recv)` from indexed receivers (strings / arrays) to
//! the stateful iterator cells the C4+ method-call surface mints
//! (`m.keys()` / `m.values()` / `m.entries()` on an `any` Map/Set,
//! `arr.values()` boxed into `any`).
//!
//! Dispatch tree:
//! - **Strings / arrays** (ShortStr immediate, `Tag::Str`,
//!   `Tag::Arr`) — indexed tier. `*idx_slot` is the cursor; the
//!   element read reuses [`__torajs_any_index_get`], the bound
//!   re-reads [`__torajs_any_iter_len`] every step (ES §23.1.5.1
//!   ArrayIterator re-reads length live; mid-loop pushes are
//!   visited). Strings step per UTF-16 code unit — same documented
//!   deviation from per-code-point iteration as the RFC's S5 note.
//! - **`Tag::MapIter` / `Tag::ArrIter`** — the cell carries its own
//!   cursor; route through the `*_iter_step` kernels. Step payloads
//!   come out borrowed (ENTRIES pair arrays pre-decremented to 0),
//!   so `payload_rc_inc` converts to owned before boxing — the same
//!   ledger the C4+ `next()` arm uses.
//! - anything else — catchable TypeError (ES §7.4.3 GetIterator on
//!   a non-iterable), returns 0 so the loop body never runs.
//!
//! Return contract: 1 = `*out` holds an owned AnyValue (heap cells
//! +1, released by the loop var's scope drop); 0 = done, `*out` is
//! `undefined`.

use core::ffi::c_void;

use crate::index_any::{__torajs_any_index_get, __torajs_any_iter_len};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::payload_rc_inc;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-collections — MapIter cursor advance (out pair is a
    /// borrow; ENTRIES pair arrays come back pre-decremented to 0).
    fn __torajs_map_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-arr — ArrIter cursor advance (same out-pair contract).
    fn __torajs_arr_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout; `idx_slot` / `out` are valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next(
    recv: AnyValue,
    idx_slot: *mut i64,
    out: *mut AnyValue,
) -> i64 {
    let cell_tag = if is_cell(recv) {
        Some(unsafe { (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read() })
    } else {
        None
    };
    let indexed = is_short_str(recv)
        || matches!(cell_tag, Some(t) if t == Tag::Str as u16 || t == Tag::Arr as u16);
    if indexed {
        unsafe {
            let idx = *idx_slot;
            if idx >= __torajs_any_iter_len(recv) {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            *idx_slot = idx + 1;
            *out = __torajs_any_index_get(recv, idx);
        }
        return 1;
    }
    type StepFn = unsafe extern "C" fn(*mut c_void, *mut i64, *mut i64) -> i64;
    let step: Option<StepFn> = match cell_tag {
        Some(t) if t == Tag::MapIter as u16 => Some(__torajs_map_iter_step),
        Some(t) if t == Tag::ArrIter as u16 => Some(__torajs_arr_iter_step),
        _ => None,
    };
    if let Some(step_fn) = step {
        let mut tag = 0i64;
        let mut payload = 0i64;
        unsafe {
            if step_fn(as_void_ptr(recv) as *mut c_void, &mut tag, &mut payload) == 0 {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            // Step payloads are borrows — convert to owned before
            // boxing (ENTRIES pre-decrement lands the pair at 1).
            payload_rc_inc(tag, payload);
            *out = __torajs_anyv_box_from_pair(tag, payload);
        }
        return 1;
    }
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
        *out = VALUE_UNDEFINED;
    }
    0
}
