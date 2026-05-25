//! WeakMap substrate — pointer-identity-keyed map with auto-eviction
//! when the key dies.
//!
//! Port of `runtime_weakmap.c` (P4.3'-c, 2026-05-24). Each entry
//! registers itself in the shared observer registry under the key,
//! so the `__torajs_weakref_target_dying` broadcast walks back into
//! `__torajs_weakmap_invalidate_key` to drop the entry when the key
//! is reclaimed. The map holds a **strong** ref on the value; only
//! the key is observed weakly.
//!
//! ## Heap layout (24 bytes)
//!
//! ```text
//!   offset 0  : universal heap header (8B)
//!   offset 8  : n_buckets    (u32)
//!   offset 12 : n_entries    (u32)
//!   offset 16 : buckets      (*mut *mut WeakMapEntry)
//! ```
//!
//! ## Entry layout (24 bytes)
//!
//! ```text
//!   offset 0  : key   (*mut c_void) — observed, NOT rc'd
//!   offset 8  : value (*mut c_void) — strong-rc'd while in the map
//!   offset 16 : next  (*mut WeakMapEntry) — hash-bucket chain
//! ```
//!
//! ## Load-factor + grow
//!
//! `n_buckets` doubles when `(n_entries + 1) * 4 > n_buckets * 3`
//! (i.e. load > 0.75). Rebuild rehash — bounded sizes here make
//! incremental rehash unnecessary. Initial bucket count is 16
//! (must be power-of-2 for the hash mask).

use core::ffi::c_void;
use core::ptr;

use crate::layout::{HeapHeader, OBSERVER_WEAKMAP, TAG_WEAKMAP};
use crate::registry::{__torajs_weakref_registry_deregister, __torajs_weakref_registry_register};

/// Initial bucket count. Power-of-2 (required by `hash_ptr_for`'s
/// `n - 1` mask). Matches `runtime_weakmap.c::WEAKMAP_INITIAL_BUCKETS`.
const WEAKMAP_INITIAL_BUCKETS: u32 = 16;

/// `STATIC_LITERAL` flag bit — must match
/// `crate::layout::FLAG_STATIC_LITERAL`. Repeated here to keep `drop`
/// dependency-tight (no inter-module use across helper sweeps).
const FLAG_STATIC_LITERAL: u16 = 4;

/// Mirror of `runtime_weakmap.c::WeakMapEntry`. Single-linked-list
/// node in a bucket chain.
#[repr(C)]
struct WeakMapEntry {
    key: *mut c_void,
    value: *mut c_void,
    next: *mut WeakMapEntry,
}

/// Mirror of `runtime_weakmap.c::WeakMap`. ABI-shared with the C-side
/// `WeakMap` struct (which still appears in code via TAG dispatch in
/// runtime_str.c::value_drop_heap → __torajs_weakmap_drop).
#[repr(C)]
struct WeakMap {
    header: HeapHeader,
    n_buckets: u32,
    n_entries: u32,
    buckets: *mut *mut WeakMapEntry,
}

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_calloc"]
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// Defined in `torajs-rc` (libtorajs_rc.a); rc-aware retain.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Defined in `runtime_str.c` — universal-drop dispatcher.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Fold a pointer into a bucket index. Same splitmix-style mix as
/// `crate::registry::hash_ptr`, but parameterized on `n_buckets`
/// (per-WeakMap sizing).
#[inline]
fn hash_ptr_for(p: *mut c_void, n_buckets: u32) -> u32 {
    let mut v = p as usize;
    v ^= v >> 33;
    v = v.wrapping_mul(0xff51_afd7_ed55_8ccd);
    v ^= v >> 33;
    v = v.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    v ^= v >> 33;
    (v as u32) & (n_buckets - 1)
}

/// Walk bucket `bkt`'s chain looking for the entry with key `key`.
/// Returns NULL on miss.
///
/// # Safety
/// `m.buckets` must be a valid `*WeakMapEntry`-array of length
/// `m.n_buckets`. `bkt < m.n_buckets`.
#[inline]
unsafe fn find(m: *mut WeakMap, key: *mut c_void, bkt: u32) -> *mut WeakMapEntry {
    let mut cur = unsafe { *(*m).buckets.add(bkt as usize) };
    while !cur.is_null() {
        if unsafe { (*cur).key } == key {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }
    ptr::null_mut()
}

/// Double `n_buckets` + rehash every entry into the fresh array.
/// Called from `set` when load exceeds 0.75. Old `buckets` array
/// is libc-free'd after migration.
///
/// # Safety
/// Same as `find` — `m` is a live WeakMap. After return, `m.buckets`
/// is the new array and `m.n_buckets` doubled.
#[inline]
unsafe fn grow(m: *mut WeakMap) {
    let old_n = unsafe { (*m).n_buckets };
    let old = unsafe { (*m).buckets };
    let new_n = old_n * 2;
    let next_buckets = unsafe { calloc(new_n as usize, core::mem::size_of::<*mut WeakMapEntry>()) }
        as *mut *mut WeakMapEntry;
    for i in 0..old_n as usize {
        let mut cur = unsafe { *old.add(i) };
        while !cur.is_null() {
            let next = unsafe { (*cur).next };
            let bkt = hash_ptr_for(unsafe { (*cur).key }, new_n);
            unsafe {
                (*cur).next = *next_buckets.add(bkt as usize);
                *next_buckets.add(bkt as usize) = cur;
            }
            cur = next;
        }
    }
    unsafe { free(old as *mut c_void) };
    unsafe {
        (*m).buckets = next_buckets;
        (*m).n_buckets = new_n;
    }
}

// ============================================================
// Public (C-callable) API.
// ============================================================

/// `new WeakMap()` — allocate a fresh empty map. Initial `n_buckets`
/// = 16; buckets array zero-init via `calloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_create() -> *mut c_void {
    let m = unsafe { malloc(core::mem::size_of::<WeakMap>()) } as *mut WeakMap;
    unsafe {
        (*m).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_WEAKMAP,
            flags: 0,
        };
        (*m).n_buckets = WEAKMAP_INITIAL_BUCKETS;
        (*m).n_entries = 0;
        (*m).buckets = calloc(
            WEAKMAP_INITIAL_BUCKETS as usize,
            core::mem::size_of::<*mut WeakMapEntry>(),
        ) as *mut *mut WeakMapEntry;
    }
    m as *mut c_void
}

/// `m.set(key, value)` — install or replace. Replacing drops the old
/// value's strong ref before installing the new one. NULL `m` or NULL
/// `key` is a defensive silent no-op (the typechecker normally
/// rejects, but we don't trust caller bugs).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_set(
    p: *mut c_void,
    key: *mut c_void,
    value: *mut c_void,
) {
    if p.is_null() || key.is_null() {
        return;
    }
    let m = p as *mut WeakMap;
    if unsafe { ((*m).n_entries + 1) * 4 > (*m).n_buckets * 3 } {
        unsafe { grow(m) };
    }
    let bkt = hash_ptr_for(key, unsafe { (*m).n_buckets });
    let existing = unsafe { find(m, key, bkt) };
    if !existing.is_null() {
        let old_val = unsafe { (*existing).value };
        if !old_val.is_null() {
            unsafe { __torajs_value_drop_heap(old_val) };
        }
        if !value.is_null() {
            unsafe { __torajs_rc_inc(value) };
        }
        unsafe { (*existing).value = value };
        return;
    }
    let e = unsafe { malloc(core::mem::size_of::<WeakMapEntry>()) } as *mut WeakMapEntry;
    unsafe {
        (*e).key = key;
        if !value.is_null() {
            __torajs_rc_inc(value);
        }
        (*e).value = value;
        (*e).next = *(*m).buckets.add(bkt as usize);
        *(*m).buckets.add(bkt as usize) = e;
        (*m).n_entries += 1;
        __torajs_weakref_registry_register(key, OBSERVER_WEAKMAP, m as *mut c_void);
    }
}

/// `m.get(key)` — return the value with rc_inc, or NULL when absent.
/// Caller takes ownership of the returned strong ref.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_get(p: *mut c_void, key: *mut c_void) -> *mut c_void {
    if p.is_null() || key.is_null() {
        return ptr::null_mut();
    }
    let m = p as *mut WeakMap;
    let bkt = hash_ptr_for(key, unsafe { (*m).n_buckets });
    let e = unsafe { find(m, key, bkt) };
    if e.is_null() {
        return ptr::null_mut();
    }
    let v = unsafe { (*e).value };
    if !v.is_null() {
        unsafe { __torajs_rc_inc(v) };
    }
    v
}

/// `m.has(key)` — 1 / 0 as i64 (SSA-side bool widens to i64).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_has(p: *mut c_void, key: *mut c_void) -> i64 {
    if p.is_null() || key.is_null() {
        return 0;
    }
    let m = p as *mut WeakMap;
    let bkt = hash_ptr_for(key, unsafe { (*m).n_buckets });
    if unsafe { find(m, key, bkt) }.is_null() {
        0
    } else {
        1
    }
}

/// `m.delete(key)` — returns 1 if key was present. Drops the value's
/// strong ref + frees the entry + deregisters from the observer
/// registry (so a later dying-key broadcast skips this map).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_delete(p: *mut c_void, key: *mut c_void) -> i64 {
    if p.is_null() || key.is_null() {
        return 0;
    }
    let m = p as *mut WeakMap;
    let bkt = hash_ptr_for(key, unsafe { (*m).n_buckets });
    let mut slot: *mut *mut WeakMapEntry = unsafe { (*m).buckets.add(bkt as usize) };
    while !unsafe { *slot }.is_null() {
        let cur = unsafe { *slot };
        if unsafe { (*cur).key } == key {
            unsafe {
                *slot = (*cur).next;
                if !(*cur).value.is_null() {
                    __torajs_value_drop_heap((*cur).value);
                }
                free(cur as *mut c_void);
                (*m).n_entries -= 1;
                __torajs_weakref_registry_deregister(key, OBSERVER_WEAKMAP, m as *mut c_void);
            }
            return 1;
        }
        slot = unsafe { &raw mut (*cur).next };
    }
    0
}

/// Called by the shared registry's `target_dying` broadcast when a
/// key registered against this WeakMap is being reclaimed. Removes
/// the entry **without** deregistering (the registry cell is being
/// torn down anyway, so re-touching it would be wasted work).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_invalidate_key(p: *mut c_void, dying_key: *mut c_void) {
    if p.is_null() || dying_key.is_null() {
        return;
    }
    let m = p as *mut WeakMap;
    let bkt = hash_ptr_for(dying_key, unsafe { (*m).n_buckets });
    let mut slot: *mut *mut WeakMapEntry = unsafe { (*m).buckets.add(bkt as usize) };
    while !unsafe { *slot }.is_null() {
        let cur = unsafe { *slot };
        if unsafe { (*cur).key } == dying_key {
            unsafe {
                *slot = (*cur).next;
                if !(*cur).value.is_null() {
                    __torajs_value_drop_heap((*cur).value);
                }
                free(cur as *mut c_void);
                (*m).n_entries -= 1;
            }
            return;
        }
        slot = unsafe { &raw mut (*cur).next };
    }
}

/// rc-aware drop. Called from `value_drop_heap`'s TAG_WEAKMAP case
/// when a WeakMap's refcount transitions to zero. Walks every entry,
/// drops its value's strong ref + deregisters from the observer
/// registry, frees the entry, then the buckets array, then the map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakmap_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let m = p as *mut WeakMap;
    unsafe {
        if (*m).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        (*m).header.refcount -= 1;
        if (*m).header.refcount != 0 {
            return;
        }
        for i in 0..(*m).n_buckets as usize {
            let mut cur = *(*m).buckets.add(i);
            while !cur.is_null() {
                let next = (*cur).next;
                if !(*cur).value.is_null() {
                    __torajs_value_drop_heap((*cur).value);
                }
                __torajs_weakref_registry_deregister(
                    (*cur).key,
                    OBSERVER_WEAKMAP,
                    m as *mut c_void,
                );
                free(cur as *mut c_void);
                cur = next;
            }
        }
        free((*m).buckets as *mut c_void);
        free(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_c() {
        // WeakMapEntry: 3×ptr = 24B, align 8.
        assert_eq!(core::mem::size_of::<WeakMapEntry>(), 24);
        assert_eq!(core::mem::align_of::<WeakMapEntry>(), 8);
        assert_eq!(core::mem::offset_of!(WeakMapEntry, key), 0);
        assert_eq!(core::mem::offset_of!(WeakMapEntry, value), 8);
        assert_eq!(core::mem::offset_of!(WeakMapEntry, next), 16);

        // WeakMap: header(8) + 2×u32(8) + ptr(8) = 24B, align 8.
        assert_eq!(core::mem::size_of::<WeakMap>(), 24);
        assert_eq!(core::mem::align_of::<WeakMap>(), 8);
        assert_eq!(core::mem::offset_of!(WeakMap, header), 0);
        assert_eq!(core::mem::offset_of!(WeakMap, n_buckets), 8);
        assert_eq!(core::mem::offset_of!(WeakMap, n_entries), 12);
        assert_eq!(core::mem::offset_of!(WeakMap, buckets), 16);
    }

    #[test]
    fn hash_in_bounds_power_of_two() {
        // Smoke: hash mask is `n - 1`, must be in-range for power-of-2 n.
        for &n in &[16u32, 32, 64, 1024] {
            for &p in &[0x1usize, 0xdead_beef, 0xffff_ffff_ffff_fff0] {
                let h = hash_ptr_for(p as *mut c_void, n);
                assert!(h < n);
            }
        }
    }
}
