//! DynObj allocation.
//!
//! Port of `runtime_str.c::__torajs_dynobj_alloc` (P4.2-a, 2026-05-23).
//!
//! Fresh `+1`-rc heap block, header initialized, all buckets zeroed
//! (zero `key_ptr` = empty bucket per the probe contract). Subsequent
//! sub-steps (P4.2-b..-f) port the rest of the dynobj surface; until
//! then, set / probe / resize / drop continue to live in C and resolve
//! `__torajs_dynobj_alloc` via cross-tier link.

use core::ffi::c_void;

use crate::layout::{
    DYNOBJ_CAP_OFF, DYNOBJ_COUNT_OFF, DYNOBJ_HDR_SIZE, DYNOBJ_INITIAL_CAP, DYNOBJ_TOMB_OFF,
    TAG_DYNOBJ,
};

unsafe extern "C" {
    /// libc calloc — zero-init alloc, ensures every bucket's `key_ptr`
    /// starts as NULL (empty) which the probe contract requires.
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
}

/// `__torajs_dynobj_alloc()` — allocate a fresh empty dynobj.
///
/// Block size: `DYNOBJ_HDR_SIZE + DYNOBJ_INITIAL_CAP * DYNOBJ_BUCKET_SIZE`
/// = 24 + 8 × 24 = 216 bytes. Header is initialized to `refcount = 1`,
/// `type_tag = TAG_DYNOBJ`, `flags = 0`. `count = 0`, `cap = 8`,
/// `tomb = 0`. Returns a fresh `+1`-rc heap pointer.
///
/// # Safety
/// Returned pointer is owned by the caller; release via
/// `__torajs_dynobj_drop` (still in C runtime_str.c until P4.2-f).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut c_void {
    let cap = DYNOBJ_INITIAL_CAP;
    let bytes = DYNOBJ_HDR_SIZE + (cap as usize) * crate::layout::DYNOBJ_BUCKET_SIZE;
    let p = unsafe { calloc(1, bytes) } as *mut u8;
    unsafe {
        // Header init: rc=1, tag=DynObj, flags=0.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_DYNOBJ;
        *(p.add(6) as *mut u16) = 0;
        // count = 0 (already zeroed by calloc — explicit for clarity)
        *(p.add(DYNOBJ_COUNT_OFF) as *mut u32) = 0;
        // cap = INITIAL_CAP
        *(p.add(DYNOBJ_CAP_OFF) as *mut u32) = cap;
        // tomb = 0 (already zeroed by calloc — explicit for clarity)
        *(p.add(DYNOBJ_TOMB_OFF) as *mut u32) = 0;
    }
    p as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header + metadata fields land at the expected offsets, with
    /// initial-cap = 8 power-of-2, count/tomb zero.
    #[test]
    fn alloc_inits_header_and_metadata() {
        let p = unsafe { __torajs_dynobj_alloc() } as *mut u8;
        assert!(!p.is_null());
        unsafe {
            assert_eq!(*(p as *const u32), 1, "refcount");
            assert_eq!(*(p.add(4) as *const u16), TAG_DYNOBJ, "type_tag");
            assert_eq!(*(p.add(6) as *const u16), 0, "flags");
            assert_eq!(*(p.add(DYNOBJ_COUNT_OFF) as *const u32), 0, "count");
            assert_eq!(*(p.add(DYNOBJ_CAP_OFF) as *const u32), 8, "cap");
            assert_eq!(*(p.add(DYNOBJ_TOMB_OFF) as *const u32), 0, "tomb");

            // Buckets zero-init contract: probe relies on NULL key_ptr.
            for i in 0..DYNOBJ_INITIAL_CAP as usize {
                let bucket = p.add(DYNOBJ_HDR_SIZE + i * crate::layout::DYNOBJ_BUCKET_SIZE);
                assert_eq!(*(bucket as *const *mut c_void), core::ptr::null_mut());
                assert_eq!(*(bucket.add(8) as *const u64), 0);
                assert_eq!(*(bucket.add(16) as *const u64), 0);
            }

            // Hand back to libc (no drop helper ported yet).
            unsafe extern "C" {
                fn free(p: *mut c_void);
            }
            free(p as *mut c_void);
        }
    }
}
