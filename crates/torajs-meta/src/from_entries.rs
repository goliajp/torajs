//! `Object.fromEntries(entries)` with a runtime entries array
//! (chunk 693, T-09) — the dynamic complement to the compile-time
//! folds (static pairs literal → ObjectLit, chunk 690; struct-
//! annotated let fast-path, T-09.c). Walks the pairs array and
//! builds a dynobj per ES §20.1.2.7 (insertion order, last
//! duplicate key wins).
//!
//! Slot reads ride `__torajs_arr_get_any_boxed` — kind-aware (a
//! typed 8-byte block reboxes per the header's elem kind, an
//! FLAG_ARR_ANY block reads the NaN-box slot directly) and
//! OOB-safe (`[["a"]]` answers `{ a: undefined }`), so every
//! entries shape (`Arr<Arr<Any>>`, `Arr<Any>`, `Arr<Arr<Str>>`,
//! …) takes the same walk.
//!
//! Two passes: validate first (undefined / null / non-array
//! receivers and non-array entries throw a catchable TypeError
//! BEFORE anything allocates), then construct (no throw path — no
//! half-built dynobj leaks under the pending-throw model).

use core::ffi::{c_char, c_void};

use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, heap_type_tag, is_cell_imm};

const TAG_ARR: u16 = 2;
const ANY_HEAP: u64 = 4;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_rc_inc(p: *mut c_void);
}

const ARR_LEN_OFF: usize = 8;

/// The receiver's live Arr cell pointer, or `None` for anything
/// that isn't one (immediates, other heap tags).
unsafe fn arr_cell(v: u64) -> Option<*const c_void> {
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

/// `Object.fromEntries(entries)` — builds a fresh dynobj from a
/// runtime pairs array, returned as a cell-encoded AnyValue (+1 rc
/// from `dynobj_alloc`, transferred to the caller).
///
/// # Safety
///
/// `entries` carries a valid AnyValue bit pattern (a raw Arr
/// pointer from a statically typed receiver satisfies the cell
/// encoding).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_from_entries(entries: u64) -> u64 {
    if entries == VALUE_UNDEFINED_IMM || entries == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries requires an iterable argument".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    }
    let Some(outer) = (unsafe { arr_cell(entries) }) else {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries argument is not iterable".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    };
    // SAFETY: live Arr cell; len lives at ARR_LEN_OFF per layout.
    let len = unsafe { (outer.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    // Pass 1 — validate every entry is itself an array BEFORE any
    // allocation, so the throw path leaves nothing half-built.
    for i in 0..len {
        let entry = unsafe { __torajs_arr_get_any_boxed(outer, i) };
        if unsafe { arr_cell(entry) }.is_none() {
            // SAFETY: NUL-terminated static C string.
            unsafe {
                __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
            }
            return VALUE_UNDEFINED_IMM;
        }
    }
    // Pass 2 — construct. No throw path from here on.
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    for i in 0..len {
        let entry = unsafe { __torajs_arr_get_any_boxed(outer, i) };
        // Pass 1 proved this is an Arr cell.
        let inner = entry as *const c_void;
        let k_av = unsafe { __torajs_arr_get_any_boxed(inner, 0) };
        let v_tag = unsafe { __torajs_arr_get_any_tag(inner, 1) };
        let v_val = unsafe { __torajs_arr_get_any_value(inner, 1) };
        // ToPropertyKey → string (owned temp this walk drops).
        let k_str = unsafe { __torajs_anyv_to_str(k_av) };
        if v_tag == ANY_HEAP && v_val != 0 {
            // The dynobj slot owns its share (gOPD descriptor shape).
            // SAFETY: ANY_HEAP slot holds a valid heap pointer.
            unsafe { __torajs_rc_inc(v_val as *mut c_void) };
        }
        unsafe { __torajs_dynobj_set(&mut obj, k_str as *const u8, v_tag, v_val) };
        unsafe { __torajs_str_drop(k_str as *mut u8) };
    }
    obj as u64
}
