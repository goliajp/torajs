//! ArrIter — stateful iterator returned by `arr.keys() / .values() /
//! .entries()` for `Array<Any>` sources.
//!
//! Historically this lived in `runtime_map.c` (the C-side was added
//! when MapIter graduated to its own substrate and ArrIter
//! piggy-backed off the same file). P4.1 closed without lifting it;
//! P4.3-g (2026-05-24) finally puts it in the right crate.
//!
//! Same shape as `torajs-collections::iter::MapIter` — distinct
//! `TAG_ARR_ITER = 17` so `value_drop_heap` routes correctly.
//!
//! Currently restricted to `Array<Any>` (16B slot stride). Typed
//! `Array<T>` for non-Any `T` needs an elem-tag field + per-tag step
//! path (P5.4 follow-up). Source array layout (mirror of
//! `runtime_str.c`):
//! ```text
//! offset 0  : universal heap header (8B)
//! offset 8  : len (u64)
//! offset 16 : cap (u32) + head_offset (u32)
//! offset 24 : slots[cap] — 16B each (tag u64 + payload u64)
//! ```

use core::ffi::c_void;

use torajs_rc::HeapHeader;

use crate::layout::ARR_SLOTS_OFF;

/// `type_tag` for ArrIter heap blocks (matches `torajs_rc::Tag::ArrIter`
/// = 17).
pub const TAG_ARR_ITER: u16 = 17;

/// Iteration kind.
pub const ARR_ITER_KEYS: u32 = 0;
pub const ARR_ITER_VALUES: u32 = 1;
pub const ARR_ITER_ENTRIES: u32 = 2;

/// ANY-slot tags (mirror of `torajs_rc::AnySlotTag`; duplicated here
/// because torajs-arr currently doesn't import all of them and an
/// extra crate-wide use would over-couple just for iter's needs).
const ANY_I64: u8 = 2;
const ANY_HEAP: u8 = 4;
const ANY_UNDEF: u8 = 5;

/// Array<Any> per-slot stride.
const ANY_SLOT_BYTES: usize = 16;

/// ArrIter heap block — 32 bytes, ABI-shared with the C-side
/// definition we just deleted.
#[repr(C)]
struct ArrIter {
    header: HeapHeader,
    arr: *mut c_void,
    cursor: i64,
    kind: u32,
    _pad: u32,
}

unsafe extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — same crate, but the IR emission uses an `extern
    /// "C"` call so we keep the boundary explicit for consistency
    /// with the rest of the iter externs.
    fn __torajs_arr_alloc_any(cap: u64) -> *mut c_void;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut c_void;
}

/// Internal: alloc + init a fresh ArrIter struct. rc_inc the source
/// array so iteration stays valid past caller-side binding drop.
unsafe fn create_with_kind(arr_p: *mut c_void, kind: u32) -> *mut c_void {
    let it = unsafe { malloc(core::mem::size_of::<ArrIter>()) } as *mut ArrIter;
    unsafe {
        (*it).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_ARR_ITER,
            flags: 0,
        };
        (*it).arr = arr_p;
        (*it).cursor = 0;
        (*it).kind = kind;
        (*it)._pad = 0;
        if !arr_p.is_null() {
            __torajs_rc_inc(arr_p);
        }
    }
    it as *mut c_void
}

/// `__torajs_arr_iter_create_keys(arr)` — KEYS-kind iterator.
///
/// # Safety
/// `arr_p` is null or a live Array<Any> heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_keys(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_KEYS) }
}

/// `__torajs_arr_iter_create_values(arr)` — VALUES-kind iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_values(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_VALUES) }
}

/// `__torajs_arr_iter_create_entries(arr)` — ENTRIES `[index, value]`
/// iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_create_entries(arr_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(arr_p, ARR_ITER_ENTRIES) }
}

/// `__torajs_arr_iter_step(iter, *out_tag, *out_payload)` — advance
/// the cursor + fill out-params per kind. Returns 1 on hit, 0 when
/// cursor has run past `arr.length`.
///
/// Heap payloads in VALUES / ENTRIES come WITHOUT rc_inc — caller's
/// `__torajs_any_box` wrap rc_incs (same contract as map_iter_step).
/// ENTRIES kind builds a fresh `[index, value]` Array<Any> per step
/// and pre-decrements its refcount to 0 so the any_box wrap lands
/// at exactly 1 owner (the IteratorResult.value box).
///
/// # Safety
/// `iter_p` is null or a live ArrIter. `out_*` are valid writable
/// pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_step(
    iter_p: *mut c_void,
    out_tag: *mut i64,
    out_payload: *mut i64,
) -> i64 {
    if iter_p.is_null() {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let it = iter_p as *mut ArrIter;
    let arr = unsafe { (*it).arr };
    if arr.is_null() {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let len = unsafe { *((arr as *const u8).add(8) as *const u64) };
    let i = unsafe { (*it).cursor } as u32;
    if i as u64 >= len {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let slot_base =
        unsafe { (arr as *const u8).add(ARR_SLOTS_OFF + (i as usize) * ANY_SLOT_BYTES) };
    let slot_tag = unsafe { *(slot_base as *const u64) };
    let slot_val = unsafe { *(slot_base.add(8) as *const u64) };
    unsafe { (*it).cursor = (i + 1) as i64 };

    let (tag, payload) = match unsafe { (*it).kind } {
        k if k == ARR_ITER_KEYS => (ANY_I64 as i64, i as i64),
        k if k == ARR_ITER_VALUES => (slot_tag as i64, slot_val as i64),
        k if k == ARR_ITER_ENTRIES => {
            // Yield `[index, value]` Array<Any>; same pre-dec idiom
            // as MapIter's make_pair_arr.
            unsafe {
                let mut out_arr = __torajs_arr_alloc_any(2);
                // Index — primitive i64, no rc_inc.
                out_arr = __torajs_arr_push_any(out_arr, ANY_I64 as u64, i as u64);
                // Value — heap payload needs rc_inc before push.
                if (slot_tag & 0xff) == ANY_HEAP as u64 && slot_val != 0 {
                    __torajs_rc_inc(slot_val as *mut c_void);
                }
                out_arr = __torajs_arr_push_any(out_arr, slot_tag, slot_val);
                // Pre-decrement so any_box's inc lands at 1.
                let hdr = out_arr as *mut HeapHeader;
                (*hdr).refcount -= 1;
                (ANY_HEAP as i64, out_arr as i64)
            }
        }
        _ => (ANY_UNDEF as i64, 0),
    };

    unsafe {
        *out_tag = tag;
        *out_payload = payload;
    }
    1
}

/// `__torajs_arr_iter_drop(iter)` — rc-aware drop. Releases strong
/// ref on source array + frees iter struct.
///
/// # Safety
/// `iter_p` is null or a live ArrIter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_iter_drop(iter_p: *mut c_void) {
    if iter_p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(iter_p) } == 0 {
        return;
    }
    let it = iter_p as *mut ArrIter;
    unsafe {
        let arr = (*it).arr;
        if !arr.is_null() {
            __torajs_value_drop_heap(arr);
        }
        free(iter_p);
    }
}
