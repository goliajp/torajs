//! Insertion-order iteration surface + ES property-order visit
//! sequence.
//!
//! The dense entry array IS the insertion order, so raw iteration is
//! a plain indexed walk: callers loop `i in 0..iter_len(obj)` and
//! skip holes (`iter_key` returns NULL). For user-visible faces
//! (print / `Object.keys` / `for-in`) ES §10.1.11.1 requires
//! array-index keys first in ascending numeric order —
//! [`__torajs_dynobj_iter_order`] materializes that visit sequence
//! over the dense indices. Consumers: property printing (torajs-arr
//! print / this crate's print_any), future keys / for-in lowering.
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

/// Str-cell layout mirror for the array-index key scan (same
/// constants layout.rs asserts for key hashing).
const KEY_STR_LEN_OFF: usize = 8;
const KEY_STR_DATA_OFF: usize = 16;
const KEY_STR_FLAG_IS_LATIN1: u16 = 0x0002;
const KEY_HDR_FLAGS_OFF: usize = 6;

/// ES array-index upper bound — §6.1.7: an array index is a
/// canonical numeric string in `[0, 2^32 - 2]`.
const MAX_ARRAY_INDEX: u64 = 4_294_967_294;

/// Parse a key Str cell as a canonical array index (§6.1.7): all
/// digits, no leading zero (except `"0"` itself), value ≤ 2^32-2.
/// Handles both Latin-1 and UTF-16 payloads (digit-only keys can
/// arrive in either encoding). `None` for every other shape.
unsafe fn key_array_index(key: *const c_void) -> Option<u64> {
    let p = key as *const u8;
    let len = unsafe { *(p.add(KEY_STR_LEN_OFF) as *const u32) } as usize;
    if len == 0 || len > 10 {
        return None;
    }
    let flags = unsafe { *(p.add(KEY_HDR_FLAGS_OFF) as *const u16) };
    let is_latin1 = flags & KEY_STR_FLAG_IS_LATIN1 != 0;
    let mut val: u64 = 0;
    for i in 0..len {
        let cu = if is_latin1 {
            u16::from(unsafe { *p.add(KEY_STR_DATA_OFF + i) })
        } else {
            unsafe { *(p.add(KEY_STR_DATA_OFF + i * 2) as *const u16) }
        };
        if !(cu >= b'0' as u16 && cu <= b'9' as u16) {
            return None;
        }
        val = val * 10 + (cu - b'0' as u16) as u64;
    }
    // canonical form: no leading zero unless the key is exactly "0"
    let first = if is_latin1 {
        unsafe { *p.add(KEY_STR_DATA_OFF) }
    } else {
        (unsafe { *(p.add(KEY_STR_DATA_OFF) as *const u16) }) as u8
    };
    if len > 1 && first == b'0' {
        return None;
    }
    if val > MAX_ARRAY_INDEX {
        return None;
    }
    Some(val)
}

/// `__torajs_dynobj_iter_order(obj, out, cap)` — ES property-order
/// visit sequence (§10.1.11.1 OrdinaryOwnPropertyKeys): array-index
/// keys first in ascending numeric order, then the remaining keys
/// in insertion order. Holes are excluded, so callers loop
/// `j in 0..n` over `out[j]` dense-entry indices without a NULL-key
/// check (enumerable filtering stays caller-side per the iter
/// contract). Fills `out[0..n]`, returns `n`; `cap` must be ≥
/// `iter_len(obj)` (short caps answer 0 defensively, same bar as
/// the NULL / foreign-tag inputs).
///
/// Cold path (print / keys faces): the integer prefix is built by
/// insertion sort over `(index_value << 32) | entry_idx` packed
/// u64s — value in the high bits makes the packed compare the
/// numeric compare; both halves fit (value ≤ 2^32-2, dense entry
/// count is u32-bounded).
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header;
/// `out` is null or valid for `cap` u64 writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_iter_order(
    obj: *const c_void,
    out: *mut u64,
    cap: u64,
) -> u64 {
    if obj.is_null() || out.is_null() {
        return 0;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return 0;
    }
    let len = unsafe { entries_len(obj) } as u64;
    if cap < len {
        return 0;
    }
    let mut n_int = 0usize;
    // pass 1 — array-index keys, packed (value<<32 | idx), kept
    // ascending by insertion sort (tiny n, cold path).
    for i in 0..len {
        let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        let key = bucket_key_ptr(kp_tagged);
        if let Some(val) = unsafe { key_array_index(key) } {
            let packed = (val << 32) | i;
            let mut j = n_int;
            while j > 0 && unsafe { *out.add(j - 1) } > packed {
                unsafe { *out.add(j) = *out.add(j - 1) };
                j -= 1;
            }
            unsafe { *out.add(j) = packed };
            n_int += 1;
        }
    }
    // unpack the integer prefix back to bare entry indices
    for j in 0..n_int {
        unsafe { *out.add(j) &= 0xFFFF_FFFF };
    }
    // pass 2 — remaining keys, insertion order
    let mut n = n_int;
    for i in 0..len {
        let kp_tagged = unsafe { (*entries(obj).add(i as usize)).key_ptr_tagged };
        if kp_tagged == DYNOBJ_KEY_HOLE {
            continue;
        }
        let key = bucket_key_ptr(kp_tagged);
        if unsafe { key_array_index(key) }.is_none() {
            unsafe { *out.add(n) = i };
            n += 1;
        }
    }
    n as u64
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

    /// Latin-1-flagged Str block for the array-index scan tests
    /// (`make_str` leaves flags 0 = UTF-16 semantics).
    fn make_latin1_str(s: &str) -> Vec<u64> {
        let v = make_str(s);
        unsafe {
            let p = v.as_ptr() as *mut u8;
            *(p.add(6) as *mut u16) = 0x0002;
        }
        v
    }

    /// Mixed integer / string keys: order = array indices ascending
    /// ("0", "2", "10"), then the rest in insertion order ("beta",
    /// "alpha", "01", "4294967295" — non-canonical / out-of-range
    /// forms stay string keys). Holes excluded.
    #[test]
    fn iter_order_es_property_order() {
        let kb = make_latin1_str("beta");
        let k10 = make_latin1_str("10");
        let k2 = make_latin1_str("2");
        let ka = make_latin1_str("alpha");
        let k0 = make_latin1_str("0");
        let klz = make_latin1_str("01");
        let kmax = make_latin1_str("4294967295");
        let keys: Vec<*const c_void> = [&kb, &k10, &k2, &ka, &k0, &klz, &kmax]
            .iter()
            .map(|v| v.as_ptr() as *const c_void)
            .collect();
        unsafe {
            let obj = __torajs_dynobj_alloc();
            for (i, &k) in keys.iter().enumerate() {
                raw_insert(obj, k, (i as u64 + 1) * 11);
            }
            let len = __torajs_dynobj_iter_len(obj);
            assert_eq!(len, 7);
            let mut out = vec![0u64; len as usize];
            let n = __torajs_dynobj_iter_order(obj, out.as_mut_ptr(), len);
            assert_eq!(n, 7);
            // integer prefix ascending: "0"(idx 4), "2"(idx 2), "10"(idx 1)
            assert_eq!(&out[..3], &[4, 2, 1]);
            // rest insertion order: "beta"(0), "alpha"(3), "01"(5), "4294967295"(6)
            assert_eq!(&out[3..], &[0, 3, 5, 6]);

            // hole exclusion: delete "2" → prefix shrinks, no NULL slots
            raw_delete(obj, keys[2]);
            let n2 = __torajs_dynobj_iter_order(obj, out.as_mut_ptr(), len);
            assert_eq!(n2, 6);
            assert_eq!(&out[..2], &[4, 1]);

            // short cap / NULL out answer 0 defensively
            assert_eq!(__torajs_dynobj_iter_order(obj, out.as_mut_ptr(), 2), 0);
            assert_eq!(
                __torajs_dynobj_iter_order(obj, core::ptr::null_mut(), len),
                0
            );

            free(obj, block_bytes(DYNOBJ_INITIAL_CAP));
        }
    }

    /// UTF-16-encoded digit key still classifies as an array index.
    #[test]
    fn iter_order_utf16_digit_key() {
        // make_str leaves flags 0 (UTF-16); write "7" as one LE u16.
        let mut v = vec![0u64; 4];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            *(p.add(STR_LEN_OFF) as *mut u64) = 1;
            *(p.add(STR_DATA_OFF) as *mut u16) = b'7' as u16;
        }
        let ks = make_latin1_str("s");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            raw_insert(obj, ks.as_ptr() as *const c_void, 1);
            raw_insert(obj, v.as_ptr() as *const c_void, 2);
            let mut out = [0u64; 2];
            let n = __torajs_dynobj_iter_order(obj, out.as_mut_ptr(), 2);
            assert_eq!(n, 2);
            assert_eq!(out, [1, 0]);
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
