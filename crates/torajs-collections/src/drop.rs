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
    /// torajs-mmalloc libc-compat free — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    /// torajs-anyvalue — NaN-box AnyValue decoders.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); gated on
    /// `FLAG_SUBCLASSED` so plain maps/sets never call out.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
}

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 0) — exotic cell minted as a user-class instance.
const FLAG_SUBCLASSED: u16 = 1;

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
            let k_anyv = (*e).key_anyv;
            if __torajs_anyv_unbox_tag(k_anyv) as u8 == ANY_HEAP {
                let kp = __torajs_anyv_unbox_value(k_anyv) as *mut c_void;
                if !kp.is_null() {
                    __torajs_value_drop_heap(kp);
                }
            }
            let v_anyv = (*e).value_anyv;
            if __torajs_anyv_unbox_tag(v_anyv) as u8 == ANY_HEAP {
                let vp = __torajs_anyv_unbox_value(v_anyv) as *mut c_void;
                if !vp.is_null() {
                    __torajs_value_drop_heap(vp);
                }
            }
        }
        if (*m).header.flags & FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(p);
        }
        // Own-property bag (§24.1.6 ordinary-object face) — the
        // universal dispatcher routes it to the dynobj drop.
        if !(*m).props.is_null() {
            __torajs_value_drop_heap((*m).props);
            (*m).props = core::ptr::null_mut();
        }
        free((*m).slots as *mut c_void);
        free((*m).entries as *mut c_void);
        free(p);
    }
}

/// `__torajs_set_drop(s)` — Set is layout-identical to Map (same
/// `entries[]` / `slots[]` shape, Set's `value_anyv` is always
/// ANY_UNDEF so the value-side `__torajs_value_drop_heap` walk is a
/// no-op). Routed via `value_drop_heap`'s `Tag::Set` case; delegates
/// straight to `__torajs_map_drop` for the actual walk + free.
///
/// # Safety
/// `p` is null or a live Set heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_set_drop(p: *mut c_void) {
    unsafe { __torajs_map_drop(p) }
}
