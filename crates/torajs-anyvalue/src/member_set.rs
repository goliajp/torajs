//! `__torajs_any_member_set` — `recv.key = value` where the receiver
//! is an `any` value (the write mirror of the chunk-508 member-read
//! fallback).
//!
//! Pre-tag-dispatch the lowering handed every `any` receiver straight
//! to `__torajs_dynobj_set`, which reads the cell as a DynObj layout —
//! a RegExp / Str / Arr receiver was silent memory corruption (probed:
//! `r.lastIndex = 3` scrambles the regex cell; `s.x = 1` SIGSEGVs).
//! This entry gates on the heap tag first:
//!
//! - `Tag::DynObj` → the ordinary `dynobj_set` path. The receiver
//!   slot is updated in place when the set relocates the block
//!   (realloc-on-grow), re-encoded as a fresh NaN-box cell.
//! - `Tag::RegExp` + the interned `lastIndex` hint → the typed
//!   tier's `regex_set_last_index` kernel (numeric payloads only;
//!   other payload tags raise the boundary TypeError below).
//! - `Tag::Arr` → `arrprops_set` expando (lazily-allocated props
//!   dynobj). `length` writes surface the boundary TypeError — the
//!   truncation semantics are a recorded follow-up, and an expando
//!   shadow would be silent-wrong (reads answer the real length).
//! - everything else (other cells, primitives, null/undefined) →
//!   catchable TypeError, never a blind layout write.
//!
//! Argument ledger: `(tag, value)` arrives with the lowering's +1 on
//! heap payloads (the consume convention `dynobj_set` expects); the
//! non-consuming arms release that reference before throwing.

use core::ffi::c_void;

use torajs_rc::{ANY_RPROP_LAST_INDEX, ANY_WPROP_ARR_LENGTH, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    /// torajs-dynobj — realloc-on-grow set; the slot receives the
    /// possibly-relocated block pointer.
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// torajs-arr — expando write through the lazily-allocated
    /// props dynobj.
    fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *const c_void, tag: i64, value: i64);
    /// torajs-regex — `re.lastIndex` setter.
    fn __torajs_regex_set_last_index(re: *mut c_void, idx: f64);
    /// torajs-dynobj — fresh empty table for the first closure
    /// expando write.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Release the lowering's +1 on a heap payload that no arm consumed.
unsafe fn drop_payload(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(4, value as i64)) };
    }
}

unsafe fn reject(tag: u64, value: u64) {
    unsafe {
        drop_payload(tag, value);
        __torajs_throw_type_error(c"cannot assign to a property of this any value".as_ptr());
    }
}

/// See module doc. `hint` carries the compile-time member-name
/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const MEMBER_SET_CLOSURE_PROPS_OFF: usize = 24;

/// intern: `ANY_RPROP_LAST_INDEX` for `lastIndex`,
/// `ANY_WPROP_ARR_LENGTH` for `length`, −1 otherwise.
///
/// # Safety
/// `recv_slot` points at a live AnyValue slot the caller owns; cell
/// receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
) {
    unsafe {
        let recv = *recv_slot;
        if !is_cell(recv) {
            reject(tag, value);
            return;
        }
        let ptr = as_void_ptr(recv);
        let cell_tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if cell_tag == Tag::DynObj as u16 {
            let mut obj = ptr;
            __torajs_dynobj_set(&mut obj, key, tag, value);
            if obj != ptr {
                // Relocated block — the NaN-box cell encoding is the
                // pointer bits; transfer, no rc traffic (same object
                // identity, moved storage).
                *recv_slot = __torajs_anyv_box_from_pair(4, obj as i64);
            }
            return;
        }
        if cell_tag == Tag::RegExp as u16 && hint == ANY_RPROP_LAST_INDEX {
            let idx = match tag {
                2 => value as i64 as f64,
                // The f64 slot stores fractional values uncoerced
                // (`r.lastIndex = 2.9` reads back 2.9); ToLength
                // happens at the regex kernels' consumption sites.
                3 => f64::from_bits(value),
                _ => {
                    // Non-numeric lastIndex payloads are a recorded
                    // boundary (ES stores any value; the cell field
                    // is f64) — loud, not a silent 0.
                    reject(tag, value);
                    return;
                }
            };
            __torajs_regex_set_last_index(ptr, idx);
            return;
        }
        if cell_tag == Tag::Closure as u16 {
            let props_slot = ptr.cast::<u8>().add(MEMBER_SET_CLOSURE_PROPS_OFF) as *mut u64;
            let mut props = *props_slot as *mut c_void;
            if props.is_null() {
                props = __torajs_dynobj_alloc();
            }
            __torajs_dynobj_set(&mut props, key, tag, value);
            // First-write alloc and resize relocation both land the
            // fresh table back in the +24 slot; the closure cell
            // itself never moves.
            *props_slot = props as u64;
            return;
        }
        if cell_tag == Tag::Arr as u16 && hint != ANY_WPROP_ARR_LENGTH {
            __torajs_arrprops_set(ptr, key, tag as i64, value as i64);
            return;
        }
        reject(tag, value);
    }
}
