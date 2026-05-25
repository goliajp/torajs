//! `Array<Any>` substrate — tagged 16-byte slots.
//!
//! Port of `runtime_str.c` lines 414-582 (P4.1-d, 2026-05-23).
//!
//! Layout: same 24-byte header (refcount/type_tag/flags + len + cap),
//! but slot stride is 16 bytes (vs 8 for `Array<T>`):
//!
//! ```text
//! [hdr 24][slot0: tag u64 + value u64][slot1: ...]
//! ```
//!
//! `flags` carries `FLAG_ARR_ANY` so:
//! - `arr_free` routes the block out of the regular cap-matched pool
//!   (whose 8-byte-stride assumption doesn't match)
//! - `arr_drop_any` is the correct walker (cf. `arr_drop`)
//!
//! `type_tag` stays `TAG_ARR` so the universal heap-walker (rc_inc /
//! rc_dec / cycle detector) treats it like any other array; the
//! Any-vs-T dispatch happens at the codegen call site.
//!
//! `head_offset` stays 0 for Any-arrays — they never deque-shift
//! (T-13.5 head trick is regular-Array-only).
//!
//! ## Public surface
//!
//! - [`__torajs_arr_alloc_any`] — fresh empty Array<Any> with `cap`
//! - [`__torajs_arr_alloc_any_filled`] — `new Array(n)`, len=cap=n,
//!   all slots ANY_NULL (zeroed)
//! - [`__torajs_arr_push_any`] — append (tag, value); grow 2× on full
//! - [`__torajs_arr_extend_any`] — append every slot of src; rc_inc on
//!   ANY_HEAP slots; grow if needed
//! - [`__torajs_arr_get_any_tag`] / [`__torajs_arr_get_any_value`] —
//!   OOB-safe reads (ANY_UNDEF / 0)
//! - [`__torajs_arr_set_any`] — indexed write; rc_dec old slot if it
//!   was ANY_HEAP

use core::ffi::c_void;

use torajs_rc::FLAG_ARR_ANY;

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF, TAG_ARR};

/// Tag value for ANY_HEAP — slot's `value` is a refcounted heap ptr.
const ANY_HEAP: u64 = 4;

/// Tag value for ANY_UNDEF — returned by OOB get to match JS spec.
const ANY_UNDEF: u64 = 5;

/// 16 bytes — Array<Any> slot stride.
const ANY_SLOT_BYTES: usize = 16;

/// Cap slot offset (matches torajs-arr::alloc's `ARR_CAP_LOW32_OFF`).
const ARR_CAP_LOW32_OFF: usize = 16;
const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_realloc"]
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;

    /// Cross-tier — torajs-rc. Increments refcount; NULL pass-through.
    fn __torajs_rc_inc(p: *mut c_void);

    /// Cross-tier — runtime_str.c's universal heap value dropper.
    /// Used by set_any to release the previous slot when overwriting
    /// an ANY_HEAP entry.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

#[inline]
unsafe fn slot_tag_ptr(arr: *mut u8, i: u64) -> *mut u64 {
    unsafe { arr.add(ARR_SLOTS_OFF + (i as usize) * ANY_SLOT_BYTES) as *mut u64 }
}

#[inline]
unsafe fn slot_val_ptr(arr: *mut u8, i: u64) -> *mut u64 {
    unsafe { arr.add(ARR_SLOTS_OFF + (i as usize) * ANY_SLOT_BYTES + 8) as *mut u64 }
}

#[inline]
unsafe fn write_header_any(p: *mut u8, len: u64, cap: u32) {
    unsafe {
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0; // Any-arrays never deque-shift
    }
}

/// `__torajs_arr_alloc_any(cap)` — fresh empty Array<Any>.
/// Bypasses the regular Array<T> pool (different slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any(cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_SLOTS_OFF + (cap as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, 0, cap as u32);
        p
    }
}

/// `__torajs_arr_alloc_any_filled(n)` — `new Array(n)` per ES spec
/// §23.1.2.1. len=cap=n, all slots zeroed (tag=ANY_NULL=0, value=0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8 {
    unsafe {
        let total = ARR_SLOTS_OFF + (n as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, n, n as u32);
        if n > 0 {
            core::ptr::write_bytes(p.add(ARR_SLOTS_OFF), 0, (n as usize) * ANY_SLOT_BYTES);
        }
        p
    }
}

/// Append a tagged slot. Grows 2× on `len == cap` (matches C
/// arr_push's growth strategy). Returns the (possibly-realloc'd) array
/// pointer; caller stores it back into the binding slot, mirroring the
/// `arr_push` contract.
///
/// # Safety
/// `arr` must be a valid Array<Any> heap pointer (FLAG_ARR_ANY set,
/// 16-byte slot stride). For ANY_HEAP slots the caller MUST have
/// pre-rc-incremented the heap value; push takes ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8 {
    let mut arr = arr as *mut u8;
    unsafe {
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        let cap = *(arr.add(ARR_CAP_LOW32_OFF) as *const u32);
        if (len as u32) == cap {
            let new_cap: u32 = if cap == 0 { 4 } else { cap * 2 };
            let total = ARR_SLOTS_OFF + (new_cap as usize) * ANY_SLOT_BYTES;
            arr = realloc(arr as *mut c_void, total) as *mut u8;
            *(arr.add(ARR_CAP_LOW32_OFF) as *mut u32) = new_cap;
        }
        *slot_tag_ptr(arr, len) = tag;
        *slot_val_ptr(arr, len) = value;
        *(arr.add(ARR_LEN_OFF) as *mut u64) = len + 1;
        arr
    }
}

/// Extend `dst` with `src`'s tagged slots. Both are Array<Any>
/// (16-byte slots). Each appended slot's ANY_HEAP value gets its
/// refcount bumped so dst shares ownership; src retains its own.
/// Reallocs dst when cap is insufficient (2× growth).
///
/// # Safety
/// Both `dst` and `src` must be valid Array<Any> heap pointers.
/// Caller MUST capture the return value (dst may have moved).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_extend_any(dst: *mut u8, src: *const u8) -> *mut u8 {
    let mut dst = dst;
    unsafe {
        let dst_len = *(dst.add(ARR_LEN_OFF) as *const u64);
        let src_len = *(src.add(ARR_LEN_OFF) as *const u64);
        if src_len == 0 {
            return dst;
        }
        let cap = *(dst.add(ARR_CAP_LOW32_OFF) as *const u32);
        let needed = dst_len + src_len;
        if needed > cap as u64 {
            let mut new_cap: u32 = if cap == 0 { 4 } else { cap };
            while (new_cap as u64) < needed {
                new_cap *= 2;
            }
            let total = ARR_SLOTS_OFF + (new_cap as usize) * ANY_SLOT_BYTES;
            dst = realloc(dst as *mut c_void, total) as *mut u8;
            *(dst.add(ARR_CAP_LOW32_OFF) as *mut u32) = new_cap;
        }
        for i in 0..src_len {
            // src is technically *const u8 but slot_tag_ptr / slot_val_ptr
            // share the same offset math; cast via *mut is safe as long
            // as we only read.
            let src_mut = src as *mut u8;
            let tag = *slot_tag_ptr(src_mut, i);
            let val = *slot_val_ptr(src_mut, i);
            if tag == ANY_HEAP && val != 0 {
                __torajs_rc_inc(val as *mut c_void);
            }
            *slot_tag_ptr(dst, dst_len + i) = tag;
            *slot_val_ptr(dst, dst_len + i) = val;
        }
        *(dst.add(ARR_LEN_OFF) as *mut u64) = dst_len + src_len;
        dst
    }
}

/// OOB-safe read of slot `i`'s tag. NULL arr or `i >= len` returns
/// `ANY_UNDEF=5` per ES spec §10.4.2.1 (sparse array missing-index
/// semantics).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64 {
    if arr.is_null() {
        return ANY_UNDEF;
    }
    unsafe {
        let arr = arr as *const u8;
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            return ANY_UNDEF;
        }
        *slot_tag_ptr(arr as *mut u8, i)
    }
}

/// OOB-safe read of slot `i`'s value. NULL arr or `i >= len` returns
/// 0 (paired with ANY_UNDEF tag from `get_any_tag` to spec-match
/// sparse-array reads).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64 {
    if arr.is_null() {
        return 0;
    }
    unsafe {
        let arr = arr as *const u8;
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if i >= len {
            return 0;
        }
        *slot_val_ptr(arr as *mut u8, i)
    }
}

/// Indexed write — `arr[i] = (tag, value)`. NULL arr is a no-op. OOB
/// `i` is the caller's responsibility (no bounds check, matching the
/// arr_get_any_* helpers). If the slot previously held an ANY_HEAP
/// value, drop it first to keep refcount accounting balanced.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64) {
    if arr.is_null() {
        return;
    }
    let arr = arr as *mut u8;
    unsafe {
        let old_tag = *slot_tag_ptr(arr, i);
        if old_tag == ANY_HEAP {
            let old_val = *slot_val_ptr(arr, i);
            __torajs_value_drop_heap(old_val as *mut c_void);
        }
        *slot_tag_ptr(arr, i) = tag;
        *slot_val_ptr(arr, i) = value;
    }
}
