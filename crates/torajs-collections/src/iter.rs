//! MapIter — stateful iterator returned by `m.keys() / .values() /
//! .entries()` + `s.entries()`, plus `m.forEach` helper.
//!
//! Port of `runtime_map.c::{__torajs_map_iter_next,
//! __torajs_map_iter_create_keys/values/entries/set_entries,
//! __torajs_map_iter_step, __torajs_map_iter_drop}` (P4.3-f,
//! 2026-05-24).
//!
//! ## Two surfaces
//!
//! - **`__torajs_map_iter_next`** — used by `Map.prototype.forEach` /
//!   `Set.prototype.forEach`. Caller-managed cursor (i64 stack slot).
//!   No MapIter struct; just walks `entries[]` directly. Fills (k_tag,
//!   k_payload, v_tag, v_payload) out-params per live entry.
//!
//! - **`MapIter` struct + step/drop** — used by `m.keys() / .values() /
//!   .entries()` (returns a stateful iterator). Holds a strong ref to
//!   source Map (so iteration stays valid past caller-side binding
//!   drop). 4 create variants × `iter.next()` step.
//!
//! ## ENTRIES yield: `[k, v]` Array<Any> per step
//!
//! `MAP_ITER_ENTRIES` (and `MAP_ITER_SET_ENTRIES` which yields
//! `[k, k]` per spec §24.2.3.6) builds a fresh `Array<Any>(2)` per
//! step. Pre-decrement trick: the result will be wrapped by
//! `__torajs_any_box(ANY_HEAP, arr)` which rc_inc's the payload —
//! to land at refcount=1 (single owner), we decrement before
//! returning so any_box's inc balances to 1. Same idiom as
//! C-side `map_iter_make_pair_arr`.

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, ANY_UNDEF, ENTRY_HASH_TOMBSTONE, HeapHeader, Map};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat alloc/free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-arr's Array<Any> alloc (refcount=1; 16B slots).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut c_void;
    /// Cross-tier — push an Any-tagged value into an Array<Any>; may
    /// reallocate; returns the (possibly relocated) array head.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut c_void;
}

/// `type_tag` for MapIter heap blocks (matches `torajs_rc::Tag::MapIter`
/// = 16 and the C-side `__TORAJS_TAG_MAP_ITER`).
pub const TAG_MAP_ITER: u16 = 16;

/// Iteration kind — what each step yields.
pub const MAP_ITER_KEYS: u32 = 0;
pub const MAP_ITER_VALUES: u32 = 1;
/// `Map.entries()` — yield `[k, v]` Array<Any>.
pub const MAP_ITER_ENTRIES: u32 = 2;
/// `Set.entries()` — yield `[k, k]` per spec §24.2.3.6 (callback's
/// second arg = first).
pub const MAP_ITER_SET_ENTRIES: u32 = 3;

/// MapIter heap block — 32 bytes, ABI-shared with C `MapIter`.
#[repr(C)]
struct MapIter {
    header: HeapHeader,
    map: *mut Map,
    cursor: i64,
    kind: u32,
    _pad: u32,
}

/// `__torajs_map_iter_next(m, &cursor, *out_*)` — caller-managed
/// cursor (entries[] index). `*cursor = -1` signals first call (→ 0);
/// returns 1 + fills out-params on a live entry hit; 0 when cursor
/// has run off `n_used`. Used by `forEach`.
///
/// # Safety
/// `m` is null or a live Map. `cursor`, all `out_*` are valid
/// writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_next(
    p: *const c_void,
    cursor: *mut i64,
    out_k_tag: *mut i64,
    out_k_payload: *mut i64,
    out_v_tag: *mut i64,
    out_v_payload: *mut i64,
) -> i64 {
    if p.is_null() {
        return 0;
    }
    let m = p as *const Map;
    let c = unsafe { *cursor };
    let mut i = if c == -1 { 0u32 } else { c as u32 };
    let n_used = unsafe { (*m).n_used };
    while i < n_used {
        let e = unsafe { (*m).entries.add(i as usize) };
        i += 1;
        if unsafe { (*e).hash } == ENTRY_HASH_TOMBSTONE {
            continue;
        }
        unsafe {
            *out_k_tag = (*e).key_tag as i64;
            *out_k_payload = (*e).key_payload as i64;
            *out_v_tag = (*e).value_tag as i64;
            *out_v_payload = (*e).value_payload as i64;
            *cursor = i as i64;
        }
        return 1;
    }
    unsafe { *cursor = n_used as i64 };
    0
}

/// Internal: alloc a MapIter struct with given source Map + kind.
/// rc_inc's the source Map so iteration stays valid past caller-side
/// binding drop.
unsafe fn create_with_kind(map_p: *mut c_void, kind: u32) -> *mut c_void {
    let it = unsafe { malloc(core::mem::size_of::<MapIter>()) } as *mut MapIter;
    unsafe {
        (*it).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_MAP_ITER,
            flags: 0,
        };
        (*it).map = map_p as *mut Map;
        (*it).cursor = 0;
        (*it).kind = kind;
        (*it)._pad = 0;
        if !map_p.is_null() {
            __torajs_rc_inc(map_p);
        }
    }
    it as *mut c_void
}

/// `__torajs_map_iter_create_keys(m)` — KEYS-kind iterator.
///
/// # Safety
/// `m` is null or a live Map heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_create_keys(map_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(map_p, MAP_ITER_KEYS) }
}

/// `__torajs_map_iter_create_values(m)` — VALUES-kind iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_create_values(map_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(map_p, MAP_ITER_VALUES) }
}

/// `__torajs_map_iter_create_entries(m)` — ENTRIES `[k, v]` iterator.
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_create_entries(map_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(map_p, MAP_ITER_ENTRIES) }
}

/// `__torajs_map_iter_create_set_entries(m)` — SET_ENTRIES `[k, k]`
/// iterator (per spec §24.2.3.6).
///
/// # Safety
/// Same as keys.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_create_set_entries(map_p: *mut c_void) -> *mut c_void {
    unsafe { create_with_kind(map_p, MAP_ITER_SET_ENTRIES) }
}

/// Internal: alloc a fresh `[a, b]` Array<Any>(2). Pre-decrements the
/// array refcount to 0 so the upcoming `__torajs_any_box` wrap
/// rc_inc's it back to exactly 1 (single-owner = the IteratorResult
/// .value box). Without this idiom the array would leak through
/// double-ownership (caller's local + any_box's inc).
unsafe fn make_pair_arr(t1: u8, p1: u64, t2: u8, p2: u64) -> *mut c_void {
    unsafe {
        let mut arr = __torajs_arr_alloc_any(2);
        if t1 == ANY_HEAP && p1 != 0 {
            __torajs_rc_inc(p1 as *mut c_void);
        }
        arr = __torajs_arr_push_any(arr, t1 as u64, p1);
        if t2 == ANY_HEAP && p2 != 0 {
            __torajs_rc_inc(p2 as *mut c_void);
        }
        arr = __torajs_arr_push_any(arr, t2 as u64, p2);
        // Pre-decrement to 0 — see fn doc.
        let hdr = arr as *mut HeapHeader;
        (*hdr).refcount -= 1;
        arr
    }
}

/// `__torajs_map_iter_step(iter, *out_tag, *out_payload)` — advance
/// the cursor + fill out-params with the next yielded (tag, payload)
/// per the iter's kind. Returns 1 on hit, 0 when cursor runs past
/// `n_used`. Heap payloads come WITHOUT rc_inc — caller's
/// `__torajs_any_box` wrap rc_incs.
///
/// # Safety
/// `iter_p` is null or a live MapIter. `out_*` are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_step(
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
    let it = iter_p as *mut MapIter;
    let m = unsafe { (*it).map };
    if m.is_null() {
        unsafe {
            *out_tag = ANY_UNDEF as i64;
            *out_payload = 0;
        }
        return 0;
    }
    let mut i = unsafe { (*it).cursor } as u32;
    let n_used = unsafe { (*m).n_used };
    while i < n_used {
        let e = unsafe { (*m).entries.add(i as usize) };
        i += 1;
        if unsafe { (*e).hash } == ENTRY_HASH_TOMBSTONE {
            continue;
        }
        let (tag, payload) = unsafe {
            match (*it).kind {
                k if k == MAP_ITER_KEYS => ((*e).key_tag as i64, (*e).key_payload as i64),
                k if k == MAP_ITER_VALUES => ((*e).value_tag as i64, (*e).value_payload as i64),
                k if k == MAP_ITER_ENTRIES => {
                    let arr = make_pair_arr(
                        (*e).key_tag,
                        (*e).key_payload,
                        (*e).value_tag,
                        (*e).value_payload,
                    );
                    (ANY_HEAP as i64, arr as i64)
                }
                k if k == MAP_ITER_SET_ENTRIES => {
                    let arr = make_pair_arr(
                        (*e).key_tag,
                        (*e).key_payload,
                        (*e).key_tag,
                        (*e).key_payload,
                    );
                    (ANY_HEAP as i64, arr as i64)
                }
                _ => (ANY_UNDEF as i64, 0),
            }
        };
        unsafe {
            *out_tag = tag;
            *out_payload = payload;
            (*it).cursor = i as i64;
        }
        return 1;
    }
    unsafe {
        (*it).cursor = n_used as i64;
        *out_tag = ANY_UNDEF as i64;
        *out_payload = 0;
    }
    0
}

/// `__torajs_map_iter_drop(iter)` — rc-aware drop. Releases strong
/// ref on source Map + frees iter struct. Routed via TAG_MAP_ITER
/// in value_drop_heap.
///
/// # Safety
/// `iter_p` is null or a live MapIter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_iter_drop(iter_p: *mut c_void) {
    if iter_p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(iter_p) } == 0 {
        return;
    }
    let it = iter_p as *mut MapIter;
    unsafe {
        let map = (*it).map;
        if !map.is_null() {
            __torajs_value_drop_heap(map as *mut c_void);
        }
        free(iter_p);
    }
}
