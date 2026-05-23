//! DynObj table resize — fresh allocation + live-bucket re-probe.
//!
//! Private-to-crate helper used by [`crate::set`] (and later
//! [`crate::define`]) when the load factor `(count + tomb + 1) > cap * 7/8`
//! threshold trips. C-side `__torajs_dynobj_resize` (still `static` in
//! runtime_str.c) remains the duplicate used by C-side `set` / `define`
//! until those also port (P4.2-c+ progressively shrinks the duplicate
//! surface; deleted entirely once `define` lands at P4.2-e).
//!
//! Algorithm:
//! 1. calloc fresh block sized `DYNOBJ_HDR_SIZE + new_cap * BUCKET_SIZE`.
//! 2. Copy heap header verbatim (preserves refcount + type_tag + flags).
//! 3. Init count=0 / cap=new_cap / tomb=0 in the new block.
//! 4. Walk old buckets; for each live entry (key_ptr != NULL && != tombstone),
//!    probe in the new block + copy the bucket. Tombstones drop on rehash.
//! 5. Update *obj_slot + free the old block.

use core::ffi::c_void;

use crate::layout::{DYNOBJ_BUCKET_SIZE, DYNOBJ_HDR_SIZE};
use crate::probe::{Bucket, buckets, cap, probe};

unsafe extern "C" {
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

/// Grow `*obj_slot` to a fresh block of `new_cap` buckets, rehashing
/// every live entry. Old block is freed.
///
/// # Safety
/// `obj_slot` must point at a non-NULL `*mut c_void` that itself holds
/// a live dynobj heap pointer. `new_cap` must be a power of 2 and
/// large enough to hold the live entry count (caller passes `cap * 2`
/// which the 7/8 load-factor guard ensures is sufficient).
pub(crate) unsafe fn resize(obj_slot: *mut *mut c_void, new_cap: u32) {
    let old = unsafe { *obj_slot };
    let old_cap = unsafe { cap(old) };
    let old_bk = unsafe { buckets(old) };
    // Fresh block: header(24) + new_cap * 24.
    let bytes = DYNOBJ_HDR_SIZE + (new_cap as usize) * DYNOBJ_BUCKET_SIZE;
    let p = unsafe { calloc(1, bytes) } as *mut u8;
    unsafe {
        // Header verbatim (refcount + type_tag + flags) — same 8 bytes.
        core::ptr::copy_nonoverlapping(old as *const u8, p, 8);
        // count=0 (rebuilds below), cap=new_cap, tomb=0.
        *(p.add(8) as *mut u32) = 0;
        *(p.add(12) as *mut u32) = new_cap;
        *(p.add(16) as *mut u32) = 0;
    }
    let new_obj = p as *mut c_void;
    let new_bk = unsafe { buckets(new_obj) };
    let mut live: u32 = 0;
    for i in 0..old_cap as usize {
        let kp = unsafe { (*old_bk.add(i)).key_ptr };
        if kp.is_null() || kp == crate::layout::DYNOBJ_TOMBSTONE {
            continue;
        }
        let pr = unsafe { probe(new_obj, kp as *const c_void) };
        // SAFETY: live entries in the source are guaranteed unique,
        // so probe lands on an empty slot (found=false, idx=fresh).
        unsafe {
            *new_bk.add(pr.idx as usize) = Bucket {
                key_ptr: (*old_bk.add(i)).key_ptr,
                tag: (*old_bk.add(i)).tag,
                value: (*old_bk.add(i)).value,
            };
        }
        live += 1;
    }
    unsafe {
        *(p.add(8) as *mut u32) = live;
        *obj_slot = new_obj;
        free(old);
    }
}
