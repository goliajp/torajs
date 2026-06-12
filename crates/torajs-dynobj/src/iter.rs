//! Insertion-order iteration surface.
//!
//! The dense entry array IS the insertion order, so iteration is a
//! plain indexed walk: callers loop `i in 0..iter_len(obj)` and skip
//! holes (`iter_key` returns NULL). Consumers: property printing
//! (torajs-arr print), future `Object.keys` / `for-in` lowering.
//!
//! Keys returned by [`__torajs_dynobj_iter_key`] are **borrowed** —
//! the entry keeps its owning rc share; callers that retain the key
//! beyond the dynobj's lifetime must rc-inc it themselves.

use core::ffi::c_void;

use crate::get::type_tag;
use crate::layout::{DYNOBJ_KEY_HOLE, TAG_DYNOBJ};
use crate::probe::{bucket_flags, bucket_key_ptr, entries, entries_len};

/// `__torajs_dynobj_iter_len(obj)` — dense-array iteration upper bound
/// (holes included). Returns 0 when `obj` is NULL or not a DynObj.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_len(obj: *const c_void) -> u64 {
    if obj.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    unsafe { entries_len(obj) as u64 }
}

/// `__torajs_dynobj_iter_key(obj, i)` — the `i`-th entry's key Str
/// pointer (borrowed; no rc traffic), or NULL when the entry is a
/// hole, `i` is out of bounds, or `obj` is NULL / not a DynObj.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_key(obj: *const c_void, i: u64) -> *mut c_void {
    if obj.is_null() {
        return core::ptr::null_mut();
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return core::ptr::null_mut();
    }
    if i >= unsafe { entries_len(obj) } as u64 {
        return core::ptr::null_mut();
    }
    let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
    if kp_tagged == DYNOBJ_KEY_HOLE {
        return core::ptr::null_mut();
    }
    bucket_key_ptr(kp_tagged)
}

/// `__torajs_dynobj_iter_value(obj, i)` — the `i`-th entry's NaN-box
/// AnyValue, or 0 when the entry is a hole, `i` is out of bounds, or
/// `obj` is NULL / not a DynObj. Decode via `__torajs_anyv_unbox_*`.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_value(obj: *const c_void, i: u64) -> u64 {
    if obj.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    if i >= unsafe { entries_len(obj) } as u64 {
        return 0;
    }
    let e = unsafe { entries(obj).add(i as usize) };
    if unsafe { (*e).key_ptr_tagged } == DYNOBJ_KEY_HOLE {
        return 0;
    }
    unsafe { (*e).value_anyv }
}

/// `__torajs_dynobj_iter_flags(obj, i)` — the `i`-th entry's W/E/C
/// PropertyDescriptor flags (bit 0/1/2), or 0 when the entry is a
/// hole, `i` is out of bounds, or `obj` is NULL / not a DynObj.
/// Enumerable filtering for print / for-in lives caller-side.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_flags(obj: *const c_void, i: u64) -> u64 {
    if obj.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    if i >= unsafe { entries_len(obj) } as u64 {
        return 0;
    }
    let e = unsafe { entries(obj).add(i as usize) };
    let kp_tagged = unsafe { (*e).key_ptr_tagged };
    if kp_tagged == DYNOBJ_KEY_HOLE {
        return 0;
    }
    bucket_flags(kp_tagged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::__torajs_dynobj_alloc;
    use crate::layout::{
        BUCKET_FLAGS_DEFAULT, DYNOBJ_INITIAL_CAP, IDX_TOMBSTONE, STR_DATA_OFF, STR_LEN_OFF,
        block_bytes,
    };
    use crate::probe::{
        Entry, bucket_make_key_tagged, count, entries, index_ptr, probe, set_count, set_entries_len,
    };

    unsafe extern "C" {
        #[link_name = "__torajs_free"]
        fn free(p: *mut c_void, size: usize);
    }

    /// Synthesize a Str-shaped heap block: [hdr:8][len:8][bytes].
    /// Backed by a `Vec<u64>` so the base pointer is 8-aligned — the
    /// key-ptr low-bit flag packing requires real keys' alignment.
    fn make_str(s: &str) -> Vec<u64> {
        let bytes = STR_DATA_OFF + s.len();
        let mut v = vec![0u64; bytes.div_ceil(8)];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            *(p.add(STR_LEN_OFF) as *mut u64) = s.len() as u64;
            core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_DATA_OFF), s.len());
        }
        v
    }

    /// Hand-rolled insert (the set.rs fresh path minus the cross-tier
    /// rc_inc / anyvalue externs, which are panic stubs under cargo
    /// test): append entry + point the probed slot at it.
    unsafe fn raw_insert(obj: *mut c_void, key: *const c_void, value_anyv: u64) {
        unsafe {
            let pr = probe(obj, key);
            assert!(!pr.found, "raw_insert expects a fresh key");
            let e_idx = entries_len(obj);
            *entries(obj).add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key as *mut c_void, BUCKET_FLAGS_DEFAULT),
                value_anyv,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }

    /// Hand-rolled delete (the delete.rs path minus the cross-tier
    /// str_drop / value_drop externs): hole the entry + tombstone the
    /// index slot.
    unsafe fn raw_delete(obj: *mut c_void, key: *const c_void) {
        unsafe {
            let pr = probe(obj, key);
            assert!(pr.found, "raw_delete expects a present key");
            let e = entries(obj).add(pr.entry as usize);
            (*e).key_ptr_tagged = DYNOBJ_KEY_HOLE;
            (*e).value_anyv = 0;
            *index_ptr(obj).add(pr.slot as usize) = IDX_TOMBSTONE;
            set_count(obj, count(obj) - 1);
        }
    }

    /// Insert a / b / c, delete b, then iterate: order is a, hole, c;
    /// probe still finds a + c at their dense indices; the freed key
    /// is reported absent.
    #[test]
    fn iter_preserves_insertion_order_and_skips_holes() {
        let ka = make_str("alpha");
        let kb = make_str("beta");
        let kc = make_str("gamma");
        let (pa, pb, pc) = (
            ka.as_ptr() as *const c_void,
            kb.as_ptr() as *const c_void,
            kc.as_ptr() as *const c_void,
        );
        unsafe {
            let obj = __torajs_dynobj_alloc();
            raw_insert(obj, pa, 11);
            raw_insert(obj, pb, 22);
            raw_insert(obj, pc, 33);
            assert_eq!(count(obj), 3);
            assert_eq!(__torajs_dynobj_iter_len(obj), 3);
            assert_eq!(__torajs_dynobj_iter_key(obj, 0), pa as *mut c_void);
            assert_eq!(__torajs_dynobj_iter_key(obj, 1), pb as *mut c_void);
            assert_eq!(__torajs_dynobj_iter_key(obj, 2), pc as *mut c_void);
            assert_eq!(__torajs_dynobj_iter_value(obj, 1), 22);
            assert_eq!(__torajs_dynobj_iter_flags(obj, 0), BUCKET_FLAGS_DEFAULT);

            raw_delete(obj, pb);
            assert_eq!(count(obj), 2);
            // iter_len still spans the holes; the hole reads NULL/0.
            assert_eq!(__torajs_dynobj_iter_len(obj), 3);
            assert_eq!(__torajs_dynobj_iter_key(obj, 1), core::ptr::null_mut());
            assert_eq!(__torajs_dynobj_iter_value(obj, 1), 0);
            assert_eq!(__torajs_dynobj_iter_flags(obj, 1), 0);
            // Neighbors unaffected; deleted key absent from probe.
            assert_eq!(__torajs_dynobj_iter_key(obj, 0), pa as *mut c_void);
            assert_eq!(__torajs_dynobj_iter_key(obj, 2), pc as *mut c_void);
            assert!(!probe(obj, pb).found);
            assert!(probe(obj, pa).found);
            assert!(probe(obj, pc).found);
            // Out-of-bounds reads are NULL/0, not UB.
            assert_eq!(__torajs_dynobj_iter_key(obj, 3), core::ptr::null_mut());
            assert_eq!(__torajs_dynobj_iter_value(obj, 99), 0);

            free(obj, block_bytes(DYNOBJ_INITIAL_CAP));
        }
    }

    /// NULL / non-dynobj inputs answer the zero defaults.
    #[test]
    fn iter_null_and_foreign_inputs() {
        unsafe {
            assert_eq!(__torajs_dynobj_iter_len(core::ptr::null()), 0);
            assert_eq!(
                __torajs_dynobj_iter_key(core::ptr::null(), 0),
                core::ptr::null_mut()
            );
            // A block whose type_tag != DynObj (e.g. a Str) reads 0.
            let s = make_str("imposter");
            let sp = s.as_ptr() as *const c_void;
            assert_eq!(__torajs_dynobj_iter_len(sp), 0);
            assert_eq!(__torajs_dynobj_iter_key(sp, 0), core::ptr::null_mut());
        }
    }
}
