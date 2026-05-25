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
