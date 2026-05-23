//! Open-addressing probe + key hash / equality.
//!
//! Pure-Rust internals shared by [`crate::get`] (P4.2-b) and the
//! upcoming set / define / has / delete / drop ports (P4.2-d..-f).
//!
//! `Bucket` mirrors `runtime_str.c::__torajs_dynobj_bucket_t`:
//! ```text
//! key_ptr: *Str   — NULL = empty, DYNOBJ_TOMBSTONE = deleted, else owning ptr
//! tag    : u64    — low 8 = ANY_TAG; bits 8-10 = writable/enumerable/configurable
//! value  : u64    — per-tag payload (bool / i64 / f64-bits / heap-ptr-as-u64)
//! ```
//!
//! Probe contract: linear step = 1; mask = `cap - 1` (cap is power of 2);
//! tombstones are walked past but remembered as the first insertion
//! candidate (lazy compaction on next insert).

use core::ffi::c_void;

use crate::layout::{DYNOBJ_HDR_SIZE, DYNOBJ_TOMBSTONE, STR_DATA_OFF, STR_LEN_OFF};

/// In-block bucket — 24 bytes, `#[repr(C)]` matches the C-side
/// `__torajs_dynobj_bucket_t` 1:1.
#[repr(C)]
pub(crate) struct Bucket {
    pub(crate) key_ptr: *mut c_void,
    pub(crate) tag: u64,
    pub(crate) value: u64,
}

/// Read the dynobj's `cap: u32` at offset 12.
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn cap(obj: *const c_void) -> u32 {
    unsafe { *((obj as *const u8).add(12) as *const u32) }
}

/// Pointer to the start of the bucket array. Stride is
/// `size_of::<Bucket>() = 24` (asserted by tests).
///
/// # Safety
/// `obj` must point at a live dynobj heap block.
#[inline]
pub(crate) unsafe fn buckets(obj: *const c_void) -> *mut Bucket {
    unsafe { (obj as *mut u8).add(DYNOBJ_HDR_SIZE) as *mut Bucket }
}

/// Read a Str's `len: u64` (offset 8).
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
unsafe fn str_len(key: *const c_void) -> u64 {
    unsafe { *((key as *const u8).add(STR_LEN_OFF) as *const u64) }
}

/// Pointer to a Str's inline UTF-8 payload (offset 16).
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
unsafe fn str_data(key: *const c_void) -> *const u8 {
    unsafe { (key as *const u8).add(STR_DATA_OFF) }
}

/// FNV-1a hash over the Str's UTF-8 payload. Mirrors
/// `runtime_str.c::__torajs_dynobj_hash_str` 1:1 (same FNV-1a 64-bit
/// constants; same byte order).
///
/// # Safety
/// `key` must point at a live Str heap block.
#[inline]
pub(crate) unsafe fn hash_str(key: *const c_void) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let len = unsafe { str_len(key) };
    let data = unsafe { str_data(key) };
    for i in 0..len as usize {
        h ^= unsafe { *data.add(i) } as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Compare two Str values for equality (length + byte content). Used
/// by [`probe`] for property-key equality. Pointer-identity short-
/// circuit for interned literals.
///
/// # Safety
/// `a` and `b` must each point at a live Str heap block.
#[inline]
pub(crate) unsafe fn str_eq(a: *const c_void, b: *const c_void) -> bool {
    if a == b {
        return true;
    }
    let la = unsafe { str_len(a) };
    let lb = unsafe { str_len(b) };
    if la != lb {
        return false;
    }
    let ap = unsafe { str_data(a) };
    let bp = unsafe { str_data(b) };
    let slice_a = unsafe { core::slice::from_raw_parts(ap, la as usize) };
    let slice_b = unsafe { core::slice::from_raw_parts(bp, la as usize) };
    slice_a == slice_b
}

/// Verdict from a [`probe`] walk.
pub(crate) struct Probe {
    /// Bucket index — if `found`, this is the live key's slot; if not,
    /// this is the insertion target (first tombstone, else first empty).
    pub idx: u32,
    /// True iff `key` is present in the table at `idx`.
    pub found: bool,
}

/// Walk the bucket array looking for `key`. Linear probe step = 1.
/// First reachable empty bucket terminates the walk; first tombstone
/// is remembered for insert reuse. Returns `(idx, found)`.
///
/// # Safety
/// `obj` must point at a live dynobj heap block; `key` at a live Str.
pub(crate) unsafe fn probe(obj: *const c_void, key: *const c_void) -> Probe {
    let cap = unsafe { cap(obj) };
    let bk = unsafe { buckets(obj) };
    let h = unsafe { hash_str(key) };
    let mask = cap - 1;
    let start = (h as u32) & mask;
    let mut tombstone_at: Option<u32> = None;
    for step in 0..cap {
        let idx = (start + step) & mask;
        let kp = unsafe { (*bk.add(idx as usize)).key_ptr };
        if kp.is_null() {
            return Probe {
                idx: tombstone_at.unwrap_or(idx),
                found: false,
            };
        }
        if kp == DYNOBJ_TOMBSTONE {
            if tombstone_at.is_none() {
                tombstone_at = Some(idx);
            }
            continue;
        }
        if unsafe { str_eq(kp as *const c_void, key) } {
            return Probe { idx, found: true };
        }
    }
    // Unreachable in practice: resize keeps load factor < 1.
    Probe {
        idx: tombstone_at.unwrap_or(0),
        found: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_layout_matches_c() {
        assert_eq!(core::mem::size_of::<Bucket>(), 24);
        assert_eq!(core::mem::align_of::<Bucket>(), 8);
        assert_eq!(core::mem::offset_of!(Bucket, key_ptr), 0);
        assert_eq!(core::mem::offset_of!(Bucket, tag), 8);
        assert_eq!(core::mem::offset_of!(Bucket, value), 16);
    }

    /// FNV-1a known-answer: hash of empty string = offset basis.
    #[test]
    fn hash_str_empty_is_fnv_offset_basis() {
        // Synthesize a Str-shaped block on the heap so the layout
        // reads land in valid memory: [hdr:8][len:8][data:0]. We
        // don't care about hdr contents; hash_str only reads len + data.
        let mut buf = vec![0u8; STR_DATA_OFF];
        unsafe {
            *(buf.as_mut_ptr().add(STR_LEN_OFF) as *mut u64) = 0;
        }
        let p = buf.as_ptr() as *const c_void;
        assert_eq!(unsafe { hash_str(p) }, 0xcbf29ce484222325);
    }

    /// FNV-1a known-answer: hash of `"a"` (single byte 0x61).
    #[test]
    fn hash_str_single_byte_a() {
        let mut buf = vec![0u8; STR_DATA_OFF + 1];
        unsafe {
            *(buf.as_mut_ptr().add(STR_LEN_OFF) as *mut u64) = 1;
            *buf.as_mut_ptr().add(STR_DATA_OFF) = b'a';
        }
        let p = buf.as_ptr() as *const c_void;
        // 0xcbf29ce484222325 ^ 0x61 = 0xcbf29ce484222344, then * 0x100000001b3
        let expected = (0xcbf29ce484222325u64 ^ 0x61u64).wrapping_mul(0x100000001b3);
        assert_eq!(unsafe { hash_str(p) }, expected);
    }

    /// str_eq: identical pointer short-circuit; equal-bytes match;
    /// different-len reject; equal-len different-bytes reject.
    #[test]
    fn str_eq_cases() {
        let make = |s: &str| -> Vec<u8> {
            let mut v = vec![0u8; STR_DATA_OFF + s.len()];
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
        let a = make("hello");
        let b = make("hello");
        let c = make("world");
        let d = make("hi");
        let ap = a.as_ptr() as *const c_void;
        let bp = b.as_ptr() as *const c_void;
        let cp = c.as_ptr() as *const c_void;
        let dp = d.as_ptr() as *const c_void;
        assert!(unsafe { str_eq(ap, ap) }, "identity");
        assert!(unsafe { str_eq(ap, bp) }, "equal bytes");
        assert!(!unsafe { str_eq(ap, cp) }, "different bytes, same len");
        assert!(!unsafe { str_eq(ap, dp) }, "different lens");
    }
}
