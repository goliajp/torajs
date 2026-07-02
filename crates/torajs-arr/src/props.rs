//! Inline `arr.x = v` custom properties.
//!
//! Round 4 chunk 5b (2026-06-25) — flipped from a global side-table
//! (hash bucket → dynobj per array) to an inline `props_dynobj` slot
//! stored directly in the array header at offset
//! [`crate::layout::ARR_PROPS_OFF`] (= 24). This eliminates the
//! per-call hash-bucket walk + node alloc/free pair that used to
//! dominate the regex wire-back `arr.index = N; arr.input = S` path
//! (Round 4 decomp S10+S13 = ~64 ns/match cross-tier cost).
//!
//! Arrays normally don't carry non-index properties, so the slot is
//! initialized to NULL by [`crate::alloc::__torajs_arr_alloc_pooled`] /
//! [`crate::any::write_header_any`] / [`crates/torajs-str::split::ops::
//! write_arr_header`] / stack-alloc literal emit, and the common-case
//! read becomes a single 8-byte load + null-check.
//!
//! ## Drop hook
//!
//! [`__torajs_arrprops_drop_entry`] is invoked from
//! `crate::drop::__torajs_arr_drop` / `__torajs_arr_drop_any` on
//! refcount→0. It reads the inline slot once; if non-NULL, drops the
//! dynobj via `__torajs_value_drop_heap`. Arrays that never had
//! `.x = v` written: single load + null branch → no-op.

use core::ffi::c_void;

use crate::layout::ARR_PROPS_OFF;

unsafe extern "C" {
    /// Cross-tier — runtime_str.c's dynamic-property object alloc.
    /// (Pure C for now; ports to torajs-dynobj in P4.2.)
    fn __torajs_dynobj_alloc() -> *mut c_void;

    /// Cross-tier — set a key-value entry on `*dynobj_ptr`. May
    /// reallocate the dynobj (linear-probing hash table), so it
    /// takes a `&mut *mut c_void` to write back the new pointer.
    fn __torajs_dynobj_set(dynobj_ptr: *mut *mut c_void, key: *const c_void, tag: u64, value: u64);

    /// Cross-tier — read the tag (or ANY_UNDEF=5 on miss).
    fn __torajs_dynobj_get_tag(dynobj: *mut c_void, key: *const c_void) -> u64;

    /// Cross-tier — read the value (or 0 on miss).
    fn __torajs_dynobj_get_value(dynobj: *mut c_void, key: *const c_void) -> u64;

    /// Cross-tier — universal heap value dropper. Used to release the
    /// dynobj on drop_entry.
    fn __torajs_value_drop_heap(p: *mut c_void);

    /// Cross-tier — batch exec-props attach (torajs-dynobj
    /// `attach_exec.rs`, Round 5 attack #4). Allocates the dynobj
    /// when `*slot` is NULL and writes the `index`/`input`/`groups`
    /// triple without per-key probe / rc_inc.
    fn __torajs_dynobj_attach_exec3(
        obj_slot: *mut *mut c_void,
        k_index: *mut c_void,
        index_val: i64,
        k_input: *mut c_void,
        input_ptr: i64,
        k_groups: *mut c_void,
    );
}

/// Inline slot pointer for the array's props_dynobj.
///
/// # Safety
/// `arr_ptr` must be a live array heap block (any of regular
/// `Array<T>` / `Array<Any>` / split-block — all share the 32-byte
/// header laid out by `torajs_arr::layout`).
#[inline]
unsafe fn props_slot_ptr(arr_ptr: *mut c_void) -> *mut *mut c_void {
    unsafe { (arr_ptr as *mut u8).add(ARR_PROPS_OFF) as *mut *mut c_void }
}

/// Inline-slot lookup → the array's props dynobj, or NULL when the
/// array never had a property written. Used by the print family to
/// emit the `[ 1, 2, x: 5 ]` props face.
///
/// # Safety
/// `arr_ptr` is a live array heap block (caller's typecheck ensures).
pub(crate) unsafe fn dynobj_of(arr_ptr: *mut c_void) -> *mut c_void {
    unsafe { *props_slot_ptr(arr_ptr) }
}

/// `arr.key = (tag, value)` — lazily allocate the dynobj on first
/// set, store the pointer in the inline slot, then delegate the
/// per-key write to `__torajs_dynobj_set`. The slot is passed by
/// pointer so `dynobj_set`'s realloc-on-grow path can write back
/// the new dynobj pointer without an additional store here.
///
/// # Safety
/// `arr_ptr` is an array heap pointer (lifetime ≥ the calling scope);
/// `key` is a Str pointer (rodata-baked or rc'd).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_set(
    arr_ptr: *mut c_void,
    key: *const c_void,
    tag: i64,
    value: i64,
) {
    unsafe {
        let slot = props_slot_ptr(arr_ptr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_set(slot, key, tag as u64, value as u64);
    }
}

/// Batch exec-triple attach — `arr.index = i; arr.input = s;
/// arr.groups = undefined` in one cross-tier call (Round 5 regex
/// re-decomposition attack #4). Fresh match arrays always have a
/// NULL props slot, so the dynobj side takes its no-probe fast path.
///
/// # Safety
/// `arr_ptr` is a live array heap block whose props slot is NULL or
/// a fresh empty dynobj; keys are the immortal static exec keys;
/// `input_ptr`'s rc share transfers to the entry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_attach_exec3(
    arr_ptr: *mut c_void,
    k_index: *mut c_void,
    index_val: i64,
    k_input: *mut c_void,
    input_ptr: i64,
    k_groups: *mut c_void,
) {
    unsafe {
        let slot = props_slot_ptr(arr_ptr);
        __torajs_dynobj_attach_exec3(slot, k_index, index_val, k_input, input_ptr, k_groups);
    }
}

/// Read `arr.key`'s tag, or `ANY_UNDEF=5` if not set.
///
/// # Safety
/// `arr_ptr` is a live array heap block; `key` is a Str pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_get_tag(
    arr_ptr: *mut c_void,
    key: *const c_void,
) -> u64 {
    unsafe {
        let dynobj = *props_slot_ptr(arr_ptr);
        if dynobj.is_null() {
            return 5; // ANY_UNDEF
        }
        __torajs_dynobj_get_tag(dynobj, key)
    }
}

/// Read `arr.key`'s value, or 0 if not set.
///
/// # Safety
/// `arr_ptr` is a live array heap block; `key` is a Str pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_get_value(
    arr_ptr: *mut c_void,
    key: *const c_void,
) -> u64 {
    unsafe {
        let dynobj = *props_slot_ptr(arr_ptr);
        if dynobj.is_null() {
            return 0;
        }
        __torajs_dynobj_get_value(dynobj, key)
    }
}

/// Drop hook — called from `arr_drop` / `arr_drop_any` on rc→0.
/// Reads the inline slot once; if non-NULL, releases the dynobj
/// via `__torajs_value_drop_heap`. Arrays that never had props
/// written: single load + null branch → no-op.
///
/// # Safety
/// `arr_ptr` is a live array heap block reaching rc=0 in the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_drop_entry(arr_ptr: *mut c_void) {
    unsafe {
        let slot = props_slot_ptr(arr_ptr);
        let dynobj = *slot;
        if !dynobj.is_null() {
            *slot = core::ptr::null_mut();
            __torajs_value_drop_heap(dynobj);
        }
    }
}
