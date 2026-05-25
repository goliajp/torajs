//! WeakSet substrate — pointer-identity-keyed set with auto-eviction
//! when the key dies.
//!
//! Port of `runtime_weakset.c` (P4.3'-d, 2026-05-24). Shape mirrors
//! `weakmap` minus the value side: entries hold only a key, and the
//! set holds no strong ref on anything. Each entry registers in the
//! shared observer registry under the key; on key death the
//! registry's broadcast invokes `__torajs_weakset_invalidate_key` to
//! drop the entry.
//!
//! ## Heap layout (24 bytes)
//!
//! ```text
//!   offset 0  : universal heap header (8B)
//!   offset 8  : n_buckets    (u32)
//!   offset 12 : n_entries    (u32)
//!   offset 16 : buckets      (*mut *mut WeakSetEntry)
//! ```
//!
//! ## Entry layout (16 bytes)
//!
//! ```text
//!   offset 0  : key  (*mut c_void) — observed, NOT rc'd
//!   offset 8  : next (*mut WeakSetEntry) — hash-bucket chain
//! ```
//!
//! ## Load-factor + grow
//!
//! Same as WeakMap: doubles when `(n_entries + 1) * 4 > n_buckets * 3`
//! (load > 0.75); fresh `calloc` + rehash, free old. Initial bucket
//! count 16 (power-of-2 for hash mask).

use core::ffi::c_void;
use core::ptr;

use crate::layout::{HeapHeader, OBSERVER_WEAKSET, TAG_WEAKSET};
use crate::registry::{__torajs_weakref_registry_deregister, __torajs_weakref_registry_register};

/// Initial bucket count. Power-of-2. Matches
/// `runtime_weakset.c::WEAKSET_INITIAL_BUCKETS`.
const WEAKSET_INITIAL_BUCKETS: u32 = 16;

/// `STATIC_LITERAL` flag bit — see `weakmap.rs` for rationale on
/// re-declaring locally vs cross-module use.
const FLAG_STATIC_LITERAL: u16 = 4;

/// Mirror of `runtime_weakset.c::WeakSetEntry`. 16 bytes.
#[repr(C)]
struct WeakSetEntry {
    key: *mut c_void,
    next: *mut WeakSetEntry,
}

/// Mirror of `runtime_weakset.c::WeakSet`. Same shape as
/// `weakmap::WeakMap` (4-field layout); only the entry type differs.
#[repr(C)]
struct WeakSet {
    header: HeapHeader,
    n_buckets: u32,
    n_entries: u32,
    buckets: *mut *mut WeakSetEntry,
}

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_calloc"]
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// Fold a pointer into a bucket index. Same splitmix-style mix as
/// `crate::registry::hash_ptr` and `crate::weakmap::hash_ptr_for`.
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
/// `s.buckets` must be a valid array of length `s.n_buckets`. `bkt`
/// must be < `s.n_buckets`.
#[inline]
unsafe fn find(s: *mut WeakSet, key: *mut c_void, bkt: u32) -> *mut WeakSetEntry {
    let mut cur = unsafe { *(*s).buckets.add(bkt as usize) };
    while !cur.is_null() {
        if unsafe { (*cur).key } == key {
            return cur;
        }
        cur = unsafe { (*cur).next };
    }
    ptr::null_mut()
}

/// Double `n_buckets` + rehash. Called from `add` when load > 0.75.
///
/// # Safety
/// Same as `find` — `s` is a live WeakSet.
#[inline]
unsafe fn grow(s: *mut WeakSet) {
    let old_n = unsafe { (*s).n_buckets };
    let old = unsafe { (*s).buckets };
    let new_n = old_n * 2;
    let next_buckets = unsafe { calloc(new_n as usize, core::mem::size_of::<*mut WeakSetEntry>()) }
        as *mut *mut WeakSetEntry;
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
        (*s).buckets = next_buckets;
        (*s).n_buckets = new_n;
    }
}

// ============================================================
// Public (C-callable) API.
// ============================================================

/// `new WeakSet()` — allocate a fresh empty set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_create() -> *mut c_void {
    let s = unsafe { malloc(core::mem::size_of::<WeakSet>()) } as *mut WeakSet;
    unsafe {
        (*s).header = HeapHeader {
            refcount: 1,
            type_tag: TAG_WEAKSET,
            flags: 0,
        };
        (*s).n_buckets = WEAKSET_INITIAL_BUCKETS;
        (*s).n_entries = 0;
        (*s).buckets = calloc(
            WEAKSET_INITIAL_BUCKETS as usize,
            core::mem::size_of::<*mut WeakSetEntry>(),
        ) as *mut *mut WeakSetEntry;
    }
    s as *mut c_void
}

/// `s.add(key)` — idempotent insert (second add on the same key is a
/// silent no-op; matches WeakSet spec semantics).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_add(p: *mut c_void, key: *mut c_void) {
    if p.is_null() || key.is_null() {
        return;
    }
    let s = p as *mut WeakSet;
    if unsafe { ((*s).n_entries + 1) * 4 > (*s).n_buckets * 3 } {
        unsafe { grow(s) };
    }
    let bkt = hash_ptr_for(key, unsafe { (*s).n_buckets });
    if !unsafe { find(s, key, bkt) }.is_null() {
        // Idempotent: spec says WeakSet.add is no-op on duplicate.
        return;
    }
    let e = unsafe { malloc(core::mem::size_of::<WeakSetEntry>()) } as *mut WeakSetEntry;
    unsafe {
        (*e).key = key;
        (*e).next = *(*s).buckets.add(bkt as usize);
        *(*s).buckets.add(bkt as usize) = e;
        (*s).n_entries += 1;
        __torajs_weakref_registry_register(key, OBSERVER_WEAKSET, s as *mut c_void);
    }
}

/// `s.has(key)` — 1 / 0 as i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_has(p: *mut c_void, key: *mut c_void) -> i64 {
    if p.is_null() || key.is_null() {
        return 0;
    }
    let s = p as *mut WeakSet;
    let bkt = hash_ptr_for(key, unsafe { (*s).n_buckets });
    if unsafe { find(s, key, bkt) }.is_null() {
        0
    } else {
        1
    }
}

/// `s.delete(key)` — 1 if key was present. Frees the entry +
/// deregisters from the observer registry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_delete(p: *mut c_void, key: *mut c_void) -> i64 {
    if p.is_null() || key.is_null() {
        return 0;
    }
    let s = p as *mut WeakSet;
    let bkt = hash_ptr_for(key, unsafe { (*s).n_buckets });
    let mut slot: *mut *mut WeakSetEntry = unsafe { (*s).buckets.add(bkt as usize) };
    while !unsafe { *slot }.is_null() {
        let cur = unsafe { *slot };
        if unsafe { (*cur).key } == key {
            unsafe {
                *slot = (*cur).next;
                free(cur as *mut c_void);
                (*s).n_entries -= 1;
                __torajs_weakref_registry_deregister(key, OBSERVER_WEAKSET, s as *mut c_void);
            }
            return 1;
        }
        slot = unsafe { &raw mut (*cur).next };
    }
    0
}

/// Called by the shared registry's `target_dying` broadcast when a
/// key registered against this WeakSet is being reclaimed. Removes
/// the entry without deregistering (cell is torn down by the caller).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_invalidate_key(p: *mut c_void, dying_key: *mut c_void) {
    if p.is_null() || dying_key.is_null() {
        return;
    }
    let s = p as *mut WeakSet;
    let bkt = hash_ptr_for(dying_key, unsafe { (*s).n_buckets });
    let mut slot: *mut *mut WeakSetEntry = unsafe { (*s).buckets.add(bkt as usize) };
    while !unsafe { *slot }.is_null() {
        let cur = unsafe { *slot };
        if unsafe { (*cur).key } == dying_key {
            unsafe {
                *slot = (*cur).next;
                free(cur as *mut c_void);
                (*s).n_entries -= 1;
            }
            return;
        }
        slot = unsafe { &raw mut (*cur).next };
    }
}

/// rc-aware drop. Called from `value_drop_heap`'s TAG_WEAKSET case
/// when a WeakSet's refcount transitions to zero. Walks every entry,
/// deregisters from the registry, frees the entry, then bucket array,
/// then the set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakset_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let s = p as *mut WeakSet;
    unsafe {
        if (*s).header.flags & FLAG_STATIC_LITERAL != 0 {
            return;
        }
        (*s).header.refcount -= 1;
        if (*s).header.refcount != 0 {
            return;
        }
        for i in 0..(*s).n_buckets as usize {
            let mut cur = *(*s).buckets.add(i);
            while !cur.is_null() {
                let next = (*cur).next;
                __torajs_weakref_registry_deregister(
                    (*cur).key,
                    OBSERVER_WEAKSET,
                    s as *mut c_void,
                );
                free(cur as *mut c_void);
                cur = next;
            }
        }
        free((*s).buckets as *mut c_void);
        free(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_c() {
        // WeakSetEntry: 2×ptr = 16B, align 8.
        assert_eq!(core::mem::size_of::<WeakSetEntry>(), 16);
        assert_eq!(core::mem::align_of::<WeakSetEntry>(), 8);
        assert_eq!(core::mem::offset_of!(WeakSetEntry, key), 0);
        assert_eq!(core::mem::offset_of!(WeakSetEntry, next), 8);

        // WeakSet: header(8) + 2×u32(8) + ptr(8) = 24B (same as WeakMap).
        assert_eq!(core::mem::size_of::<WeakSet>(), 24);
        assert_eq!(core::mem::align_of::<WeakSet>(), 8);
        assert_eq!(core::mem::offset_of!(WeakSet, header), 0);
        assert_eq!(core::mem::offset_of!(WeakSet, n_buckets), 8);
        assert_eq!(core::mem::offset_of!(WeakSet, n_entries), 12);
        assert_eq!(core::mem::offset_of!(WeakSet, buckets), 16);
    }
}
