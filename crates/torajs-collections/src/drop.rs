//! Map / Set universal-drop entry — `__torajs_map_drop`.
//!
//! Port of `runtime_map.c::__torajs_map_drop` (P4.3-e, 2026-05-24).
//! Routed via `value_drop_heap`'s `TAG_MAP` case when a Map's
//! refcount transitions to zero.
//!
//! Walks every live `entries[]` slot (skipping entry-side tombstones),
//! drops the heap-tagged key + value refs, then libc-frees the
//! `slots[]` array, the `entries[]` array, and the Map struct itself.
//!
//! Uses cross-tier `__torajs_rc_dec` for the decrement (matches the
//! arr/dynobj drop pattern; STATIC_LITERAL handling is folded into
//! rc_dec, so no inline flag check needed).

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, ENTRY_HASH_TOMBSTONE, Map};

unsafe extern "C" {
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn free(p: *mut c_void);
}

/// `__torajs_map_drop(m)` — refcount-aware drop. Returns immediately
/// if `m` is null, STATIC_LITERAL, or refcount stays positive after
/// decrement. On last-owner: walks live entries dropping heap refs,
/// then frees the two arrays + Map struct.
///
/// # Safety
/// `m` is null or a live Map heap pointer. After return, the pointee
/// may be freed (last-owner path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_drop(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(p) } == 0 {
        return;
    }
    let m = p as *mut Map;
    unsafe {
        let n_used = (*m).n_used;
        for i in 0..n_used as usize {
            let e = (*m).entries.add(i);
            if (*e).hash == ENTRY_HASH_TOMBSTONE {
                continue;
            }
            if (*e).key_tag == ANY_HEAP {
                let kp = (*e).key_payload as *mut c_void;
                if !kp.is_null() {
                    __torajs_value_drop_heap(kp);
                }
            }
            if (*e).value_tag == ANY_HEAP {
                let vp = (*e).value_payload as *mut c_void;
                if !vp.is_null() {
                    __torajs_value_drop_heap(vp);
                }
            }
        }
        free((*m).slots as *mut c_void);
        free((*m).entries as *mut c_void);
        free(p);
    }
}
