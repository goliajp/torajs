//! `Object.groupBy(items, callbackFn)` per ES §20.1.2.10.
//!
//! Walks `items` (Array-of-Any lane; iterable-only receivers are
//! L3b — spec step 2 requires the full IteratorRecord protocol)
//! and dispatches `callbackFn(item, index)` via the uniform
//! boxed-adapter ABI (`__torajs_any_call`). Each returned key
//! coerces to a property-key string (spec step 3 in AddValueToKeyedGroup
//! calls ToPropertyKey; the returned object is null-prototype so
//! only string keys are observable). Same-key hits push into a
//! shared `Array<Any>`; a fresh key mints one.
//!
//! Null / undefined `items` throw a catchable TypeError before any
//! allocation (spec step 2 GetIterator wraps the argument in an
//! ObjectCoercible check first). A non-callable cb surfaces its
//! throw through the first `__torajs_any_call` — the accumulator
//! built so far is released on that path.
//!
//! Ownership contract:
//! - Every `arr_get_any_boxed(items, i)` slot is BORROWED (arr
//!   keeps its share). `unbox_value_owned` mints a fresh stake for
//!   the bucket push (`arr_push_any` takes the ANY_HEAP payload by
//!   transfer).
//! - `any_call` argv slots are BORROWED (per the sibling
//!   `method_call_closure_dispatch` contract); return is an OWNED
//!   AnyValue — the walk drops it after `anyv_to_str` (which
//!   itself answers a fresh owned Str for the bucket key).
//! - Bucket pointers are stored in the result dynobj (ANY_HEAP
//!   slot). `arr_push_any` may realloc the data buffer but never
//!   moves the cell itself, so the dynobj slot remains valid.

use core::ffi::{c_char, c_void};

use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, heap_type_tag, is_cell_imm};

const ANY_HEAP: u64 = 4;
const ANY_I64: i64 = 2;
const TAG_ARR: u16 = 2;
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    fn __torajs_any_call(recv: u64, argv: *const u64, argc: i64) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Route raw AnyValue bits to a live Array cell, or `None` (nullish
/// / non-cell / other-tag).
unsafe fn arr_cell(v: u64) -> Option<*const c_void> {
    if v == VALUE_UNDEFINED_IMM || v == VALUE_NULL_IMM {
        return None;
    }
    if !is_cell_imm(v) {
        return None;
    }
    let p = v as *const c_void;
    if unsafe { heap_type_tag(p) } == TAG_ARR {
        Some(p)
    } else {
        None
    }
}

/// Release an OWNED AnyValue payload (heap cell → rc_dec; short-str
/// → drop the materialized cell it carries; inline tags → no-op).
/// Mirrors the release path the ssa-lower emits after an any-call.
unsafe fn drop_owned_any(v: u64) {
    let t = unsafe { __torajs_anyv_unbox_tag(v) };
    if t == ANY_HEAP as i64 {
        let p = unsafe { __torajs_anyv_unbox_value(v) };
        if p != 0 {
            unsafe { __torajs_value_drop_heap(p as *mut c_void) };
        }
    }
}

/// `Object.groupBy(items, cb)` — Array items lane.
///
/// Returns a freshly minted null-prototype dynobj (refcount = 1,
/// transferred to caller). On any throw path the accumulator +
/// pending key temps release, and the return is
/// `VALUE_UNDEFINED_IMM` — the caller's throw check unwinds before
/// the value is consumed.
///
/// # Safety
/// `items` and `cb` carry valid AnyValue bit patterns; caller must
/// check for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_group_by(items: u64, cb: u64) -> u64 {
    let Some(arr) = (unsafe { arr_cell(items) }) else {
        unsafe {
            __torajs_throw_type_error(c"Object.groupBy requires an iterable argument".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    };
    // SAFETY: live Arr cell — length lives at ARR_LEN_OFF per layout.
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut result = unsafe { __torajs_dynobj_alloc() };
    for i in 0..len {
        // Borrowed item — arr keeps its own share.
        let item = unsafe { __torajs_arr_get_any_boxed(arr, i) };
        // Index box (I64 tag). i is bounded by an arr length that
        // fits in u32 (per arr_alloc bounds), so an i64 cast is
        // total.
        let idx = unsafe { __torajs_anyv_box_from_pair(ANY_I64, i as i64) };
        let argv = [item, idx];
        // cb(item, i) — argv is borrowed; key is OWNED.
        let key = unsafe { __torajs_any_call(cb, argv.as_ptr(), 2) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { drop_owned_any(key) };
            unsafe { __torajs_value_drop_heap(result as *mut c_void) };
            return VALUE_UNDEFINED_IMM;
        }
        // ToPropertyKey — string coercion (owned Str temp).
        let key_str = unsafe { __torajs_anyv_to_str(key) };
        unsafe { drop_owned_any(key) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { __torajs_str_drop(key_str as *mut u8) };
            unsafe { __torajs_value_drop_heap(result as *mut c_void) };
            return VALUE_UNDEFINED_IMM;
        }
        // Lookup / mint bucket. dynobj_get_tag answers UNDEF (5) for
        // an absent key; a data slot answers ANY_HEAP + the arr
        // pointer.
        let existing_tag = unsafe { __torajs_dynobj_get_tag(result, key_str as *const u8) };
        let bucket: *mut c_void = if existing_tag != ANY_HEAP {
            let fresh = unsafe { __torajs_arr_alloc_any(4) } as *mut c_void;
            unsafe {
                __torajs_dynobj_set(&mut result, key_str as *const u8, ANY_HEAP, fresh as u64);
            }
            fresh
        } else {
            unsafe { __torajs_dynobj_get_value(result, key_str as *const u8) as *mut c_void }
        };
        // Push item — unbox_value_owned mints the +1 the bucket
        // slot keeps (arr_push_any takes the ANY_HEAP payload by
        // transfer). Inline tags round-trip through the borrowed
        // unbox_value path (no rc traffic).
        let t = unsafe { __torajs_anyv_unbox_tag(item) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(item) };
        // Bucket cell itself never moves (data buffer may realloc);
        // the dynobj slot stays valid.
        let _ = unsafe { __torajs_arr_push_any(bucket, t as u64, val as u64) };
        unsafe { __torajs_str_drop(key_str as *mut u8) };
    }
    result as u64
}
