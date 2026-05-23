//! Map key hashing — SplitMix64 + FNV-1a + Any-tag dispatch.
//!
//! Port of `runtime_map.c::{map_mix_u64, map_hash_bytes, map_hash_key}`
//! (P4.3-b, 2026-05-23). Pure-Rust internals shared by [`crate::probe`]
//! + the upcoming get/has/set/delete extern ports. C-side copies stay
//! `static` until their consumer fns port — algorithm duplicated.
//!
//! Hash invariant: the return value is **always `>= 1`**. Value 0 is
//! reserved as `ENTRY_HASH_TOMBSTONE` on the entry side (a deleted
//! entry has `MapEntry::hash == 0`), so the live-vs-dead test is just
//! "is hash non-zero".

use core::ffi::c_void;

use crate::layout::{
    ANY_BOOL, ANY_F64, ANY_HEAP, ANY_I64, ANY_NULL, ANY_UNDEF, HeapHeader, STR_DATA_OFF,
    STR_LEN_OFF, TAG_STR,
};

/// SplitMix64 finalizer — strong avalanche; same primitive V8 / Java
/// / many others use as a mixing helper. Truncates to u32 for the
/// slot hash field.
#[inline]
pub(crate) fn map_mix_u64(mut x: u64) -> u32 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x as u32
}

/// FNV-1a 64-bit hash over a byte slice, finalized through SplitMix.
///
/// # Safety
/// `bytes` must be a valid readable pointer for `len` bytes.
#[inline]
pub(crate) unsafe fn map_hash_bytes(bytes: *const u8, len: u64) -> u32 {
    let mut h: u64 = 0xcbf29ce484222325;
    for i in 0..len as usize {
        h ^= unsafe { *bytes.add(i) } as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    map_mix_u64(h)
}

/// Hash an Any-tagged key. Returned value is always `>= 1` so it
/// never collides with `ENTRY_HASH_TOMBSTONE`. Handles SameValueZero
/// canonicalization for `NaN` (all bit patterns hash the same) and
/// `+0` / `-0` (the IEEE eq predicate already says equal — hash same).
///
/// # Safety
/// For `ANY_HEAP` tag, `payload` must be either NULL or a valid live
/// heap pointer with a universal header (so the type_tag read at
/// offset 4 is sound).
pub(crate) unsafe fn map_hash_key(tag: u8, payload: u64) -> u32 {
    let h: u32 = match tag {
        ANY_NULL => 0xa5a5_a5a5,
        ANY_UNDEF => 0x5a5a_5a5a,
        ANY_BOOL => map_mix_u64(payload ^ 0xb001),
        ANY_I64 => map_mix_u64(payload ^ 0xa11),
        ANY_F64 => {
            let d = f64::from_bits(payload);
            // SameValueZero: NaN → canonical; ±0 → shared bucket.
            if d.is_nan() {
                0xdead_beef
            } else if d == 0.0 {
                0xfa57_c0de
            } else {
                map_mix_u64(payload ^ 0xa11)
            }
        }
        ANY_HEAP => {
            let p = payload as *mut c_void;
            if p.is_null() {
                0x1234_5678
            } else {
                let hdr = p as *const HeapHeader;
                let type_tag = unsafe { (*hdr).type_tag };
                if type_tag == TAG_STR {
                    let len = unsafe { *((p as *const u8).add(STR_LEN_OFF) as *const u64) };
                    let data = unsafe { (p as *const u8).add(STR_DATA_OFF) };
                    unsafe { map_hash_bytes(data, len) }
                } else {
                    // Non-Str heap: pointer-identity hash.
                    map_mix_u64((p as u64) ^ 0xf00d)
                }
            }
        }
        _ => map_mix_u64(payload ^ 0xbad),
    };
    if h == 0 { 1 } else { h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};

    #[test]
    fn mix_u64_known() {
        // SplitMix64 finalizer: 0 → 0; non-zero avalanche.
        assert_eq!(map_mix_u64(0), 0);
        assert_ne!(map_mix_u64(1), 0);
    }

    #[test]
    fn hash_key_null_undef_constants() {
        // Pure constants — no Safety preconditions to violate.
        assert_eq!(unsafe { map_hash_key(ANY_NULL, 0) }, 0xa5a5_a5a5);
        assert_eq!(unsafe { map_hash_key(ANY_UNDEF, 0) }, 0x5a5a_5a5a);
    }

    #[test]
    fn hash_key_nan_canonicalizes() {
        let nan_a = f64::NAN.to_bits();
        let mut nan_b = nan_a;
        nan_b ^= 1u64 << 50; // Different NaN bit pattern, still NaN.
        assert!(f64::from_bits(nan_b).is_nan());
        assert_eq!(unsafe { map_hash_key(ANY_F64, nan_a) }, unsafe {
            map_hash_key(ANY_F64, nan_b)
        });
    }

    #[test]
    fn hash_key_pos_neg_zero_same() {
        let pos_zero = (0.0_f64).to_bits();
        let neg_zero = (-0.0_f64).to_bits();
        assert_ne!(pos_zero, neg_zero, "bit patterns differ");
        assert_eq!(unsafe { map_hash_key(ANY_F64, pos_zero) }, unsafe {
            map_hash_key(ANY_F64, neg_zero)
        });
    }

    #[test]
    fn hash_key_never_zero() {
        // Spot check: a few inputs must all map away from 0.
        for &(t, p) in &[
            (ANY_NULL, 0),
            (ANY_UNDEF, 0),
            (ANY_BOOL, 0),
            (ANY_BOOL, 1),
            (ANY_I64, 0),
            (ANY_I64, 42),
        ] {
            assert_ne!(unsafe { map_hash_key(t, p) }, 0);
        }
    }

    #[test]
    fn hash_bytes_empty_is_fnv_basis_through_mix() {
        let empty: [u8; 0] = [];
        // FNV-1a basis 0xcbf29ce484222325 → SplitMix → some non-zero u32.
        let h = unsafe { map_hash_bytes(empty.as_ptr(), 0) };
        // Just assert it equals mix(basis).
        assert_eq!(h, map_mix_u64(0xcbf29ce484222325));
    }

    #[test]
    fn hash_key_heap_str_hashes_by_content() {
        // Synthesize two Str blocks with identical bytes "abc":
        //   [hdr:8][len:8][data:3]
        let make_str = |s: &[u8]| -> Vec<u8> {
            let mut v = vec![0u8; STR_DATA_OFF + s.len()];
            // hdr.type_tag = TAG_STR=0 at offset 4 (default zero from vec).
            unsafe {
                *(v.as_mut_ptr().add(STR_LEN_OFF) as *mut u64) = s.len() as u64;
                core::ptr::copy_nonoverlapping(
                    s.as_ptr(),
                    v.as_mut_ptr().add(STR_DATA_OFF),
                    s.len(),
                );
            }
            v
        };
        let a = make_str(b"abc");
        let b = make_str(b"abc");
        let ha = unsafe { map_hash_key(ANY_HEAP, a.as_ptr() as u64) };
        let hb = unsafe { map_hash_key(ANY_HEAP, b.as_ptr() as u64) };
        assert_eq!(ha, hb, "Str hash is content-based");
        let c = make_str(b"abd");
        let hc = unsafe { map_hash_key(ANY_HEAP, c.as_ptr() as u64) };
        assert_ne!(ha, hc, "different bytes → different hash (very likely)");
    }
}
