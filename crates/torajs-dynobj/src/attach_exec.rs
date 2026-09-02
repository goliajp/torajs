//! Batch exec-props attach — the regex wire-back's `index` / `input`
//! / `groups` triple written onto a fresh dynobj in one call.
//!
//! Round 5 regex re-decomposition attack #4. The generic
//! `__torajs_dynobj_set` path pays, per key: an FNV-1a byte hash, a
//! probe walk with `str_eq`, a resize check, an extern
//! `__torajs_rc_inc` on the key (a no-op for the wire-back's
//! `FLAG_STATIC_LITERAL` keys, but the call itself is paid) and an
//! extern NaN-box encode. Three keys per match ≈ 14 ns/iter on the
//! wire-back profile.
//!
//! This entry exploits everything the exec shape knows statically:
//!
//! * the dynobj is **fresh** (`count == 0`, straight out of
//!   [`crate::alloc::__torajs_dynobj_alloc`]) — no probe needed, the
//!   three hash slots are computed from compile-time FNV constants
//!   and collide only with each other (resolved below, still at
//!   compile time after const folding);
//! * the keys are the immortal static `"index"` / `"input"` /
//!   `"groups"` Strs — no per-entry rc_inc;
//! * the value tags are fixed (`I64` / `HEAP` / `UNDEF`) — the
//!   NaN-box encode stays on the `__torajs_anyv_box_from_pair`
//!   extern (single source of truth in torajs-anyvalue).

use core::ffi::c_void;

use crate::layout::{ANY_HEAP, ANY_UNDEF, BUCKET_FLAGS_DEFAULT, DYNOBJ_INITIAL_CAP, IDX_EMPTY};
use crate::probe::{
    Entry, bucket_make_key_tagged, count, entries, index_ptr, set_count, set_entries_len,
};

unsafe extern "C" {
    /// torajs-anyvalue — NaN-box AnyValue pair encoder (same wire as
    /// `set.rs`).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// `AnySlotTag::I64` (torajs-rc). Mirrored here like
/// [`ANY_HEAP`] / [`ANY_UNDEF`] in `layout.rs`.
const ANY_I64: u64 = 2;

/// Compile-time FNV-1a over a byte literal — must stay bit-identical
/// to [`crate::probe::hash_str`]'s runtime walk (unit-tested below).
const fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

const H_INDEX: u64 = fnv1a(b"index");
const H_INPUT: u64 = fnv1a(b"input");
const H_GROUPS: u64 = fnv1a(b"groups");

/// Attach the spec §22.2.7.8 exec triple (`index`: i64, `input`:
/// heap Str share, `groups`: undefined) to `*obj_slot`, allocating
/// the dynobj if the slot is NULL.
///
/// # Safety
///
/// * `obj_slot` points at a live `*mut c_void` that is either NULL
///   or a **fresh, empty** dynobj (`count == 0`) — the fast insert
///   writes hash slots assuming an empty table.
/// * `k_index` / `k_input` / `k_groups` are the immortal
///   `FLAG_STATIC_LITERAL` key Strs (no ownership transfer happens
///   here; entry drops on these keys are no-ops).
/// * `input_ptr`'s rc share is transferred to the entry (caller
///   rc_incs before the call, exactly like the `dynobj_set` path).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_attach_exec3(
    obj_slot: *mut *mut c_void,
    k_index: *mut c_void,
    index_val: i64,
    k_input: *mut c_void,
    input_ptr: i64,
    k_groups: *mut c_void,
) {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        obj = unsafe { crate::alloc::__torajs_dynobj_alloc() };
        unsafe { *obj_slot = obj };
    }
    debug_assert_eq!(
        unsafe { count(obj) },
        0,
        "attach_exec3 needs a fresh dynobj"
    );

    // Hash slots from the const FNV values; mutual collisions resolve
    // by the same +1 linear step as `probe`. The whole cluster const-
    // folds (mask and hashes are compile-time known).
    let mask = DYNOBJ_INITIAL_CAP - 1;
    let s0 = (H_INDEX as u32) & mask;
    let mut s1 = (H_INPUT as u32) & mask;
    while s1 == s0 {
        s1 = (s1 + 1) & mask;
    }
    let mut s2 = (H_GROUPS as u32) & mask;
    while s2 == s0 || s2 == s1 {
        s2 = (s2 + 1) & mask;
    }

    let ent = unsafe { entries(obj) };
    let idx = unsafe { index_ptr(obj) };
    unsafe {
        debug_assert_eq!(*idx.add(s0 as usize), IDX_EMPTY);
        debug_assert_eq!(*idx.add(s1 as usize), IDX_EMPTY);
        debug_assert_eq!(*idx.add(s2 as usize), IDX_EMPTY);
        *ent = Entry {
            key_ptr_tagged: bucket_make_key_tagged(k_index, BUCKET_FLAGS_DEFAULT),
            value_anyv: __torajs_anyv_box_from_pair(ANY_I64 as i64, index_val),
        };
        *ent.add(1) = Entry {
            key_ptr_tagged: bucket_make_key_tagged(k_input, BUCKET_FLAGS_DEFAULT),
            value_anyv: __torajs_anyv_box_from_pair(ANY_HEAP as i64, input_ptr),
        };
        *ent.add(2) = Entry {
            key_ptr_tagged: bucket_make_key_tagged(k_groups, BUCKET_FLAGS_DEFAULT),
            value_anyv: __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0),
        };
        *idx.add(s0 as usize) = 0;
        *idx.add(s1 as usize) = 1;
        *idx.add(s2 as usize) = 2;
        set_entries_len(obj, 3);
        set_count(obj, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{hash_str, probe};

    unsafe extern "C" {
        fn __torajs_anyv_unbox_tag(v: u64) -> i64;
        fn __torajs_anyv_unbox_value(v: u64) -> i64;
    }

    /// Build a bare Str heap block (header + len + payload) the way
    /// probe's `hash_str` / `str_eq` read it. Leaked on purpose —
    /// test-scoped.
    fn make_str(bytes: &[u8]) -> *mut c_void {
        let total = 16 + bytes.len();
        let mut buf: Vec<u8> = vec![0u8; total];
        buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        // type_tag = TAG_STR = 1 (torajs-rc HeapTag); flags = IS_LATIN1.
        buf[4..6].copy_from_slice(&1u16.to_le_bytes());
        buf[6..8].copy_from_slice(&crate::probe::KEY_STR_FLAG_IS_LATIN1.to_le_bytes());
        buf[8..16].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf[16..].copy_from_slice(bytes);
        let p = buf.as_mut_ptr() as *mut c_void;
        core::mem::forget(buf);
        p
    }

    /// The const fnv1a must stay bit-identical to probe::hash_str.
    #[test]
    fn const_fnv_matches_runtime_hash() {
        for (bytes, h) in [
            (&b"index"[..], H_INDEX),
            (&b"input"[..], H_INPUT),
            (&b"groups"[..], H_GROUPS),
        ] {
            let s = make_str(bytes);
            assert_eq!(unsafe { hash_str(s) }, h);
        }
    }

    /// End-to-end: attach on a NULL slot, then the generic probe path
    /// must find all three keys with the right tags/values.
    #[test]
    fn attach_then_generic_probe_finds_all_three() {
        let k_index = make_str(b"index");
        let k_input = make_str(b"input");
        let k_groups = make_str(b"groups");
        let fake_input: i64 = 0x6000_dead_beef;
        let mut slot: *mut c_void = core::ptr::null_mut();
        unsafe {
            __torajs_dynobj_attach_exec3(&mut slot, k_index, 7, k_input, fake_input, k_groups);
        }
        assert!(!slot.is_null());
        unsafe {
            assert_eq!(count(slot), 3);
            for (k, want_tag, want_val) in [
                (k_index, ANY_I64 as i64, 7i64),
                (k_input, ANY_HEAP as i64, fake_input),
                (k_groups, ANY_UNDEF as i64, 0),
            ] {
                let pr = probe(slot, k);
                assert!(pr.found, "key not found via generic probe");
                let e = entries(slot).add(pr.entry as usize);
                let v = (*e).value_anyv;
                assert_eq!(__torajs_anyv_unbox_tag(v), want_tag);
                if want_tag != ANY_UNDEF as i64 {
                    assert_eq!(__torajs_anyv_unbox_value(v), want_val);
                }
            }
        }
    }
}
