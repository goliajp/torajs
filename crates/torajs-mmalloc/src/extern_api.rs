//! `extern "C"` API exposed to torajs sub-crate IR call sites.
//!
//! Sub-crates currently declare `extern { fn malloc(usize) -> *mut u8 }`
//! which links against `libSystem.dylib`. v0.7-A2 step 6 will
//! search/replace those calls to use the symbols below — at link
//! time the binary will pull libtorajs_mmalloc.a instead of libc.
//!
//! Global allocator instance is `static mut` behind a spin-lock'd
//! `AtomicBool`. The user binary is single-threaded today (JS
//! spec); the spin-lock catches accidental re-entrancy + test-time
//! `cargo test` parallelism.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::large::{large_alloc, large_free};
use crate::size_class::Allocator;

static LOCK: AtomicBool = AtomicBool::new(false);
static mut ALLOC: Allocator = Allocator::new();

#[inline]
fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
fn unlock() {
    LOCK.store(false, Ordering::Release);
}

/// torajs malloc — pure-syscall replacement for libc `malloc`.
/// Returns NULL on OOM. `size == 0` returns a non-null dummy
/// pointer (matches glibc behavior; some sub-crate code paths
/// rely on never getting NULL for non-error sizes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_malloc(size: usize) -> *mut c_void {
    if size == 0 {
        return &raw const LOCK as *mut c_void; // unique non-null sentinel
    }
    if size > 4096 {
        return large_alloc(size)
            .map(|p| p as *mut c_void)
            .unwrap_or(core::ptr::null_mut());
    }
    lock();
    // SAFETY: lock held; ALLOC is a static the only mutates here.
    let p = unsafe { (*&raw mut ALLOC).alloc(size) }
        .map(|p| p as *mut c_void)
        .unwrap_or(core::ptr::null_mut());
    unlock();
    p
}

/// torajs free — sized variant. Caller MUST pass the same `size`
/// originally given to `__torajs_malloc`. (libc-compat shim that
/// auto-tracks sizes lives in a follow-up sub-step for the
/// adapter layer.)
///
/// # Safety
///
/// `ptr` must be a pointer returned by `__torajs_malloc(size)`
/// and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_free(ptr: *mut c_void, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    if size > 4096 {
        let _ = unsafe { large_free(ptr as *mut u8, size) };
        return;
    }
    lock();
    // SAFETY: lock held.
    unsafe { (*&raw mut ALLOC).dealloc(ptr as *mut u8, size) };
    unlock();
}

/// torajs realloc — `(ptr, old_size, new_size)`. Allocates a fresh
/// block, copies `min(old_size, new_size)` bytes, frees the old
/// block. Returns NULL on OOM (old block is NOT freed in that case,
/// matching glibc).
///
/// # Safety
///
/// `ptr` is null OR a valid pointer from `__torajs_malloc(old_size)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_realloc(
    ptr: *mut c_void,
    old_size: usize,
    new_size: usize,
) -> *mut c_void {
    if ptr.is_null() {
        return unsafe { __torajs_malloc(new_size) };
    }
    if new_size == 0 {
        unsafe { __torajs_free(ptr, old_size) };
        return core::ptr::null_mut();
    }
    let new_ptr = unsafe { __torajs_malloc(new_size) };
    if new_ptr.is_null() {
        return core::ptr::null_mut();
    }
    let copy_len = old_size.min(new_size);
    unsafe {
        core::ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, copy_len);
    }
    unsafe { __torajs_free(ptr, old_size) };
    new_ptr
}

// ============================================================
// libc-compat shim — auto-size tracking
//
// Sub-crate code calls `extern { fn malloc/free/realloc }` with
// the libc shape (no size on free). To migrate without rewriting
// every call site, sub-crates re-declare those externs with
// `#[link_name = "__torajs_libc_malloc"]` etc. — same Rust call
// shape, different link-time symbol → routed to these shims.
//
// Shim prepends an 8-byte size header so `free(ptr)` can recover
// the original size. Costs 8 bytes per alloc; can be replaced by
// direct __torajs_free(ptr, size) call sites later.
// ============================================================

const SHIM_HEADER: usize = 16; // 8 bytes for size + 8 bytes for alignment padding

/// libc-compat malloc. Allocates `size + SHIM_HEADER` bytes, writes
/// the size into the first 8 bytes, returns ptr offset by `SHIM_HEADER`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_malloc(size: usize) -> *mut c_void {
    let total = size + SHIM_HEADER;
    let raw = unsafe { __torajs_malloc(total) };
    if raw.is_null() {
        return core::ptr::null_mut();
    }
    // Write size at offset 0
    unsafe { core::ptr::write(raw as *mut usize, total) };
    // Return user-visible ptr (offset past header)
    unsafe { (raw as *mut u8).add(SHIM_HEADER) as *mut c_void }
}

/// libc-compat free. Reads size from prepended header, calls
/// `__torajs_free` with recovered size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let raw = unsafe { (ptr as *mut u8).sub(SHIM_HEADER) };
    let total = unsafe { core::ptr::read(raw as *const usize) };
    unsafe { __torajs_free(raw as *mut c_void, total) };
}

/// libc-compat calloc. Equivalent to `__torajs_libc_malloc(n*sz)`
/// followed by a `memset(p, 0, n*sz)` over the user-visible region.
/// Recycled free-list blocks are dirty from prior use, so the zero
/// write is unconditional — not a "fresh-from-mmap" shortcut.
///
/// Returns NULL on `n*sz` overflow or OOM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_calloc(nmemb: usize, size: usize) -> *mut c_void {
    let Some(total) = nmemb.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    let p = unsafe { __torajs_libc_malloc(total) };
    if p.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { core::ptr::write_bytes(p as *mut u8, 0, total) };
    p
}

/// libc-compat realloc. Reads old size from header, calls inner
/// `__torajs_realloc`, returns new user-visible pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_realloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    if ptr.is_null() {
        return unsafe { __torajs_libc_malloc(new_size) };
    }
    if new_size == 0 {
        unsafe { __torajs_libc_free(ptr) };
        return core::ptr::null_mut();
    }
    let raw = unsafe { (ptr as *mut u8).sub(SHIM_HEADER) };
    let old_total = unsafe { core::ptr::read(raw as *const usize) };
    let new_total = new_size + SHIM_HEADER;
    let new_raw = unsafe { __torajs_realloc(raw as *mut c_void, old_total, new_total) };
    if new_raw.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { core::ptr::write(new_raw as *mut usize, new_total) };
    unsafe { (new_raw as *mut u8).add(SHIM_HEADER) as *mut c_void }
}

/// libc-compatible memcpy. NOT overlap-safe — use `__torajs_memmove`
/// for overlapping ranges.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memcpy(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, n);
    }
    dst
}

/// libc-compatible memmove — overlap-safe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memmove(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe {
        core::ptr::copy(src as *const u8, dst as *mut u8, n);
    }
    dst
}

/// libc-compatible memcmp.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let a = unsafe { core::slice::from_raw_parts(a as *const u8, n) };
    let b = unsafe { core::slice::from_raw_parts(b as *const u8, n) };
    match a.cmp(b) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malloc_free_roundtrip() {
        let p = unsafe { __torajs_malloc(64) };
        assert!(!p.is_null());
        unsafe { core::ptr::write(p as *mut u8, 0xaa) };
        assert_eq!(unsafe { *(p as *const u8) }, 0xaa);
        unsafe { __torajs_free(p, 64) };
    }

    #[test]
    fn realloc_preserves_content() {
        let p = unsafe { __torajs_malloc(16) };
        unsafe {
            for i in 0..16 {
                core::ptr::write((p as *mut u8).add(i), i as u8);
            }
        }
        let q = unsafe { __torajs_realloc(p, 16, 32) };
        unsafe {
            for i in 0..16 {
                assert_eq!(*((q as *const u8).add(i)), i as u8);
            }
        }
        unsafe { __torajs_free(q, 32) };
    }

    #[test]
    fn large_malloc_roundtrips() {
        let p = unsafe { __torajs_malloc(16384) };
        assert!(!p.is_null());
        unsafe { core::ptr::write(p as *mut u8, 0xbb) };
        unsafe { __torajs_free(p, 16384) };
    }

    #[test]
    fn memcpy_works() {
        let src = [1u8, 2, 3, 4, 5];
        let mut dst = [0u8; 5];
        unsafe {
            __torajs_memcpy(
                dst.as_mut_ptr() as *mut c_void,
                src.as_ptr() as *const c_void,
                5,
            );
        }
        assert_eq!(dst, src);
    }

    #[test]
    fn memmove_overlap_safe() {
        let mut buf = [0u8, 1, 2, 3, 4, 5, 6, 7];
        unsafe {
            __torajs_memmove(
                buf.as_mut_ptr().add(2) as *mut c_void,
                buf.as_ptr() as *const c_void,
                4,
            );
        }
        assert_eq!(buf, [0, 1, 0, 1, 2, 3, 6, 7]);
    }

    #[test]
    fn libc_compat_malloc_free_roundtrip() {
        let p = unsafe { __torajs_libc_malloc(100) };
        assert!(!p.is_null());
        unsafe {
            for i in 0..100 {
                core::ptr::write((p as *mut u8).add(i), (i & 0xff) as u8);
            }
            for i in 0..100 {
                assert_eq!(*((p as *const u8).add(i)), (i & 0xff) as u8);
            }
            __torajs_libc_free(p);
        }
    }

    #[test]
    fn libc_compat_calloc_zeros_memory() {
        let p = unsafe { __torajs_libc_calloc(8, 16) };
        assert!(!p.is_null());
        unsafe {
            for i in 0..128 {
                assert_eq!(*((p as *const u8).add(i)), 0, "calloc byte {} not zero", i);
            }
            __torajs_libc_free(p);
        }
    }

    #[test]
    fn libc_compat_calloc_overflow_returns_null() {
        let p = unsafe { __torajs_libc_calloc(usize::MAX, 2) };
        assert!(p.is_null(), "overflow must return NULL");
    }

    #[test]
    fn libc_compat_calloc_recycled_block_still_zero() {
        // Force the second calloc to come off the free-list (size = 16
        // matches SIZE_CLASSES[0]). First alloc → write nonzero →
        // free → calloc must still see zero.
        let p1 = unsafe { __torajs_libc_malloc(16) };
        unsafe {
            for i in 0..16 {
                *((p1 as *mut u8).add(i)) = 0xff;
            }
            __torajs_libc_free(p1);
        }
        let p2 = unsafe { __torajs_libc_calloc(1, 16) };
        unsafe {
            for i in 0..16 {
                assert_eq!(*((p2 as *const u8).add(i)), 0, "recycled byte {} not zero", i);
            }
            __torajs_libc_free(p2);
        }
    }

    #[test]
    fn libc_compat_realloc_preserves_content() {
        let p = unsafe { __torajs_libc_malloc(8) };
        unsafe {
            for i in 0..8 {
                core::ptr::write((p as *mut u8).add(i), (i + 10) as u8);
            }
        }
        let q = unsafe { __torajs_libc_realloc(p, 24) };
        unsafe {
            for i in 0..8 {
                assert_eq!(*((q as *const u8).add(i)), (i + 10) as u8);
            }
            __torajs_libc_free(q);
        }
    }

    #[test]
    fn memcmp_orders_bytes() {
        assert_eq!(
            unsafe {
                __torajs_memcmp(
                    b"abc".as_ptr() as *const c_void,
                    b"abc".as_ptr() as *const c_void,
                    3,
                )
            },
            0
        );
        assert!(
            unsafe {
                __torajs_memcmp(
                    b"abc".as_ptr() as *const c_void,
                    b"abd".as_ptr() as *const c_void,
                    3,
                )
            } < 0
        );
    }
}
