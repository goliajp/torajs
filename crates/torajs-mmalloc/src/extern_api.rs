//! `extern "C"` API exposed to torajs sub-crate IR call sites.
//!
//! Phase 2a item 5 — extern_api is now a **thin shell** over the
//! Layer 1 `core::alloc_sized` / `core::free_sized` fast path.
//! The internal `Allocator` + `LOCK` statics that lived here have
//! been deleted; all allocator state lives in `core` (single
//! source of truth for the Phase 2b TLAB / Phase 2c per-CPU
//! upgrades that follow).
//!
//! Two layers exposed:
//!
//! - **Layer 1 (size-known)**: `__torajs_malloc(size)` /
//!   `__torajs_free(ptr, size)` / `__torajs_realloc(ptr, old, new)`
//!   — direct route to `core::alloc_sized` / `core::free_sized`.
//!   These are the IR-codegen and Phase-2e migration target symbols.
//!
//! - **Layer 2 (libc-compat, SHIM_HEADER-tracked)**:
//!   `__torajs_libc_malloc` / `__torajs_libc_free` /
//!   `__torajs_libc_calloc` / `__torajs_libc_realloc` — same
//!   API shape as libc malloc/free; ptr returned is offset past
//!   a 16-byte SHIM_HEADER that holds the alloc size for the
//!   size-less free contract. Retained for sub-crate callers
//!   whose Rust extern decls are `fn malloc(usize) -> *mut u8 /
//!   fn free(*mut u8)` (libc shape). Phase 2e sub-crate migration
//!   to Layer 1 sized API will retire these symbols; Phase 2f
//!   deletes them.
//!
//! Pure-function helpers `__torajs_libc_memcpy` / `memmove` /
//! `memcmp` + historical `__torajs_*` aliases hold no allocator
//! state and stay inline here.

use core::ffi::c_void;

// =============================================================
// Layer 1 — size-known fast path (route to core)
// =============================================================

/// torajs malloc — caller knows size; routes to `core::alloc_sized`.
/// Returns NULL on OOM. `size == 0` returns a non-null sentinel
/// (matches glibc behavior; sub-crate code paths rely on never
/// getting NULL for non-error sizes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_malloc(size: usize) -> *mut c_void {
    crate::core::alloc_sized(size) as *mut c_void
}

/// torajs free — caller provides original size. Routes to
/// `core::free_sized`; no SpanRegistry lookup cost.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `__torajs_malloc(size)`
/// and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_free(ptr: *mut c_void, size: usize) {
    unsafe { crate::core::free_sized(ptr as *mut u8, size) };
}

/// torajs realloc — `(ptr, old_size, new_size)`. Allocates a
/// fresh block via core, copies `min(old, new)` bytes, frees the
/// old block. Returns NULL on OOM (old block is NOT freed in that
/// case, matching glibc).
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

// =============================================================
// Layer 2 — libc-compat shim with SHIM_HEADER
// =============================================================
//
// Sub-crate code declares `extern { fn malloc(usize) -> *mut c_void }`
// (libc shape, no size on free). Sub-crates rewire via
// `#[link_name = "__torajs_libc_malloc"]` etc.; the shim below
// auto-tracks size via a 16-byte prepended SHIM_HEADER so that
// `free(ptr)` can recover the original size.
//
// Phase 2e migrates sub-crate alloc/free call sites to use Layer 1
// sized API (eliminating SHIM_HEADER per-alloc overhead). Phase 2f
// deletes this Layer 2 entirely once zero internal callers remain
// (only true external C consumers retain libc shape).

const SHIM_HEADER: usize = 16;

/// libc-compat malloc. Allocates `size + SHIM_HEADER` bytes,
/// writes the size into the first 8 bytes, returns ptr offset by
/// `SHIM_HEADER`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_malloc(size: usize) -> *mut c_void {
    let total = size + SHIM_HEADER;
    let raw = unsafe { __torajs_malloc(total) };
    if raw.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { core::ptr::write(raw as *mut usize, total) };
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
/// followed by a `memset(p, 0, n*sz)` over the user-visible
/// region. Recycled free-list blocks are dirty from prior use, so
/// the zero write is unconditional — not a "fresh-from-mmap"
/// shortcut.
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

// =============================================================
// Pure-function helpers — no allocator state
// =============================================================

/// libc-compatible memcpy. NOT overlap-safe — use
/// `__torajs_libc_memmove` for overlapping ranges. Exported under
/// two names: `__torajs_memcpy` (historical) +
/// `__torajs_libc_memcpy` (post-v0.7-A2 step 6b — IR codegen +
/// sub-crate externs both target this name now).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_memcpy(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe {
        core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, n);
    }
    dst
}

/// Historical alias — kept so existing call sites (some sub-crates,
/// older fixture-cache .o files) still resolve.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memcpy(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe { __torajs_libc_memcpy(dst, src, n) }
}

/// libc-compatible memmove — overlap-safe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_memmove(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe {
        core::ptr::copy(src as *const u8, dst as *mut u8, n);
    }
    dst
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memmove(
    dst: *mut c_void,
    src: *const c_void,
    n: usize,
) -> *mut c_void {
    unsafe { __torajs_libc_memmove(dst, src, n) }
}

/// libc-compatible memcmp.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_libc_memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let a = unsafe { core::slice::from_raw_parts(a as *const u8, n) };
    let b = unsafe { core::slice::from_raw_parts(b as *const u8, n) };
    match a.cmp(b) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    unsafe { __torajs_libc_memcmp(a, b, n) }
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
                assert_eq!(
                    *((p2 as *const u8).add(i)),
                    0,
                    "recycled byte {} not zero",
                    i
                );
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
