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
//! Map / Set sources (chunk 694) ride the caller-managed-cursor
//! `__torajs_map_iter_next` walk over the shared hash storage
//! (insertion order, tombstone-skipping — the same walk `forEach`
//! takes). A Map entry is a `(k, v)` pair by construction; a Set
//! element is the entry itself and must be a pair array (ES
//! §20.1.2.7 iterates the Set's values — primitives throw).
//!
//! Two passes: validate first (undefined / null / non-iterable
//! receivers and non-array entries throw a catchable TypeError
//! BEFORE anything allocates), then construct (no throw path — no
//! half-built dynobj leaks under the pending-throw model).

use core::ffi::{c_char, c_void};

use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, heap_type_tag, is_cell_imm};

const TAG_ARR: u16 = 2;
const TAG_MAP: u16 = 15;
const TAG_SET: u16 = 19;
const ANY_HEAP: u64 = 4;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_anyv_to_str_pair(tag: i64, value: i64) -> *mut c_void;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_map_iter_next(
        p: *const c_void,
        cursor: *mut i64,
        out_k_tag: *mut i64,
        out_k_payload: *mut i64,
        out_v_tag: *mut i64,
        out_v_payload: *mut i64,
    ) -> i64;
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
        // Map / Set receivers take the hash-storage walk instead.
        if is_cell_imm(entries) {
            let p = entries as *const c_void;
            let tag = unsafe { heap_type_tag(p) };
            if tag == TAG_MAP {
                return unsafe { from_map(p) };
            }
            if tag == TAG_SET {
                return unsafe { from_set(p) };
            }
        }
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
        unsafe { set_prop_from_pair_arr(&mut obj, entry as *const c_void) };
    }
    obj as u64
}

/// Define one property from a `[k, v]` pair array (borrowed reads;
/// OOB-safe so `[["a"]]` answers `{ a: undefined }`).
///
/// # Safety
///
/// `inner` is a live Arr cell.
unsafe fn set_prop_from_pair_arr(obj: &mut *mut c_void, inner: *const c_void) {
    let k_av = unsafe { __torajs_arr_get_any_boxed(inner, 0) };
    let v_tag = unsafe { __torajs_arr_get_any_tag(inner, 1) };
    let v_val = unsafe { __torajs_arr_get_any_value(inner, 1) };
    // ToPropertyKey → string (owned temp this walk drops).
    let k_str = unsafe { __torajs_anyv_to_str(k_av) };
    unsafe { set_prop(obj, k_str, v_tag, v_val) };
}

/// Shared define tail — the dynobj slot owns its share of a heap
/// value (gOPD descriptor shape); the key string temp is dropped.
///
/// # Safety
///
/// `k_str` is a live owned Str cell; an ANY_HEAP `v_val` holds a
/// valid heap pointer.
unsafe fn set_prop(obj: &mut *mut c_void, k_str: *mut c_void, v_tag: u64, v_val: u64) {
    if v_tag == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    unsafe { __torajs_dynobj_set(obj, k_str as *const u8, v_tag, v_val) };
    unsafe { __torajs_str_drop(k_str as *mut u8) };
}

/// Map receiver — every entry is a `(k, v)` pair by construction,
/// so there is no validate pass (no throw path → nothing
/// half-built can leak). Insertion order rides the entries[]
/// prefix walk; keys take ToPropertyKey like the array lane.
///
/// # Safety
///
/// `map` is a live Map cell.
unsafe fn from_map(map: *const c_void) -> u64 {
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    let mut cursor: i64 = -1;
    let (mut kt, mut kp, mut vt, mut vp) = (0i64, 0i64, 0i64, 0i64);
    while unsafe { __torajs_map_iter_next(map, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        let k_str = unsafe { __torajs_anyv_to_str_pair(kt, kp) };
        unsafe { set_prop(&mut obj, k_str, vt as u64, vp as u64) };
    }
    obj as u64
}

/// Set receiver — the iterated value is the element itself, which
/// must be a pair array (`new Set([["a", 1]])` is a legal entries
/// iterable; a primitive element throws per ES §20.1.2.7). Same
/// two-pass model as the array lane: validate before anything
/// allocates.
///
/// # Safety
///
/// `set` is a live Set cell (shares the Map storage layout).
unsafe fn from_set(set: *const c_void) -> u64 {
    let mut cursor: i64 = -1;
    let (mut kt, mut kp, mut vt, mut vp) = (0i64, 0i64, 0i64, 0i64);
    // Pass 1 — every element must itself be an Arr cell.
    while unsafe { __torajs_map_iter_next(set, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        let is_arr = kt == ANY_HEAP as i64
            && kp != 0
            && unsafe { heap_type_tag(kp as *const c_void) } == TAG_ARR;
        if !is_arr {
            // SAFETY: NUL-terminated static C string.
            unsafe {
                __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
            }
            return VALUE_UNDEFINED_IMM;
        }
    }
    // Pass 2 — construct. No throw path from here on.
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    cursor = -1;
    while unsafe { __torajs_map_iter_next(set, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        // Pass 1 proved every element is an Arr cell.
        unsafe { set_prop_from_pair_arr(&mut obj, kp as *const c_void) };
    }
    obj as u64
}
