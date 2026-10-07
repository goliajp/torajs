//! Core API — the two layers every allocation enters through.
//!
//! - **Layer 1** — `alloc_sized(size)` / `free_sized(ptr, size)`: the
//!   caller knows the size, so the class is two instructions and the
//!   heap's row is the whole fast path.
//! - **Layer 0** — `alloc(size)` / `free(ptr)`: the size is lost, so
//!   `free` finds the block's region in the registry; an arena then
//!   names the span and its class, a large region its length.
//!
//! By size: a small request goes through its row, a medium one to
//! its span directly (`medium.rs`), and one past `MAX_MEDIUM` maps a
//! region of its own.
//!
//! The heap itself is one process-wide `static`, `__torajs_core_tlab`,
//! whose rows the toolchain pops and pushes inline at object-cell
//! sites; everything behind a row miss is in `heap.rs`.

use crate::arena::Arena;
use crate::large::{large_alloc, large_free};
use crate::region_registry::{RegionKind, lock_addr, with_registry};
use crate::size_class::{MAX_SMALL, bucket_for, class_size, medium_class_for};
use crate::tlab::TlabCache;

// A plain `static mut`, not `#[thread_local]`: on macOS aarch64 that
// would pull `__tlv_bootstrap` and a dyld dependency into every user
// binary. The runtime is single-threaded today, so one process-wide
// heap is sound; per-thread heaps come with the threaded runtime,
// indexed by thread id by hand rather than through tlv.
//
// `#[unsafe(no_mangle)] pub`: a stable symbol so the toolchain can
// inline the row pop / push at allocation sites (`torajs-codegen`
// `compile::cell`).
#[unsafe(no_mangle)]
pub static mut __torajs_core_tlab: TlabCache = TlabCache::new();

/// Sentinel returned on zero-size alloc, so callers never confuse it
/// with a NULL that means OOM. Matches glibc.
#[inline]
fn zero_sentinel() -> *mut u8 {
    lock_addr()
}

/// Layer 1 alloc. NULL on OOM, the sentinel on `size == 0`.
///
/// `#[inline(always)]`: the row pop inlines into the caller; every
/// path past it is out of line and cold, so the fast path's frame
/// saves nothing it does not use.
#[inline(always)]
pub fn alloc_sized(size: usize) -> *mut u8 {
    #[cfg(torajs_census)]
    census::alloc(size);
    if size == 0 {
        return zero_sentinel();
    }
    if size > MAX_SMALL {
        return alloc_big(size);
    }
    let class = match bucket_for(size) {
        Some(i) => i,
        None => return core::ptr::null_mut(),
    };
    let tlab = &raw mut __torajs_core_tlab;
    if let Some(p) = unsafe { (*tlab).pop(class) } {
        return p;
    }
    alloc_refill(class)
}

/// Past the small classes: a medium span slot, or a mapping of its
/// own past `MAX_MEDIUM`.
#[cold]
#[inline(never)]
fn alloc_big(size: usize) -> *mut u8 {
    match medium_class_for(size) {
        Some(class) => {
            #[cfg(torajs_census)]
            census::MEDIUM.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let tlab = &raw mut __torajs_core_tlab;
            unsafe { (*tlab).alloc_medium(class) }
        }
        None => {
            #[cfg(torajs_census)]
            census::LARGE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            alloc_large(size)
        }
    }
}

/// Large path: a mapping of its own, registered so `free(ptr)` can
/// recover its length.
fn alloc_large(size: usize) -> *mut u8 {
    let p = match large_alloc(size) {
        Ok(p) => p,
        Err(_) => return core::ptr::null_mut(),
    };
    // large_alloc rounds to a page; the registered length must match
    let rounded = (size.max(1) + 4095) & !4095;
    with_registry(|r| r.insert(p as usize, rounded, RegionKind::Large));
    p
}

/// Row miss: the heap refills the row from the class's span.
#[cold]
#[inline(never)]
fn alloc_refill(class: usize) -> *mut u8 {
    #[cfg(torajs_census)]
    census::REFILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let tlab = &raw mut __torajs_core_tlab;
    unsafe { (*tlab).refill(class) }
}

/// Layer 1 free. The row push is the fast path; a full row spills.
///
/// # Safety
/// `ptr` came from `alloc` / `alloc_sized` with this `size` and is
/// not already freed.
#[inline(always)]
pub unsafe fn free_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr == zero_sentinel() || size == 0 {
        return;
    }
    #[cfg(torajs_census)]
    census::FREES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if size > MAX_SMALL {
        unsafe { free_big(ptr, size) };
        return;
    }
    let class = match bucket_for(size) {
        Some(i) => i,
        None => return,
    };
    let tlab = &raw mut __torajs_core_tlab;
    if unsafe { (*tlab).push(class, ptr) } {
        return;
    }
    unsafe { free_spill(ptr, class) };
}

/// Past the small classes: back to the block's span, or unmapped.
///
/// # Safety
/// As [`free_sized`], for a block past `MAX_SMALL`.
#[cold]
#[inline(never)]
unsafe fn free_big(ptr: *mut u8, size: usize) {
    match medium_class_for(size) {
        Some(class) => {
            let tlab = &raw mut __torajs_core_tlab;
            unsafe { (*tlab).free_medium(class, ptr) }
        }
        None => unsafe { free_large(ptr, size) },
    }
}

/// Large path: deregister, then unmap.
///
/// # Safety
/// As [`free_sized`], for a large allocation.
unsafe fn free_large(ptr: *mut u8, size: usize) {
    with_registry(|r| r.remove(ptr as usize));
    let _ = unsafe { large_free(ptr, size) };
}

/// Row full: the heap moves blocks back to their spans.
///
/// # Safety
/// As [`free_sized`].
#[cold]
#[inline(never)]
pub(crate) unsafe fn free_spill(ptr: *mut u8, class: usize) {
    let tlab = &raw mut __torajs_core_tlab;
    unsafe { (*tlab).spill(class, ptr) };
}

/// Layer 0 alloc: the same as `alloc_sized`, kept as its own symbol
/// for symmetry with `free`.
#[inline]
pub fn alloc(size: usize) -> *mut u8 {
    alloc_sized(size)
}

/// Layer 0 free: the registry names the block's region, the arena
/// its class.
///
/// # Safety
/// `ptr` came from `alloc` / `alloc_sized` and is not already freed.
pub unsafe fn free(ptr: *mut u8) {
    if ptr.is_null() || ptr == zero_sentinel() {
        return;
    }
    match with_registry(|r| r.lookup(ptr as usize)) {
        Some((RegionKind::Large, size)) => {
            with_registry(|r| r.remove(ptr as usize));
            let _ = unsafe { large_free(ptr, size) };
        }
        Some((RegionKind::Arena, _)) => {
            let meta = unsafe { (*Arena::containing(ptr)).meta_of(ptr) };
            let size = class_size(unsafe { (*meta).class() });
            unsafe { free_sized(ptr, size) };
        }
        // not this allocator's block: a no-op, as libc's free(NULL)
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::size_class::MAX_MEDIUM;

    #[test]
    fn alloc_sized_returns_nonnull_for_nonzero() {
        let p = alloc_sized(64);
        assert!(!p.is_null(), "alloc 64 returned null");
        unsafe {
            *p = 0xaa;
            assert_eq!(*p, 0xaa);
            free_sized(p, 64);
        }
    }

    #[test]
    fn alloc_sized_zero_returns_sentinel() {
        let p = alloc_sized(0);
        assert!(!p.is_null(), "zero-size alloc returned null");
        unsafe { free_sized(p, 0) };
    }

    #[test]
    fn alloc_sized_large_routes_to_large_alloc() {
        let big = MAX_MEDIUM + 1;
        let p = alloc_sized(big);
        assert!(!p.is_null());
        assert_eq!(
            with_registry(|r| r.lookup(p as usize)),
            Some((RegionKind::Large, (big + 4095) & !4095))
        );
        unsafe {
            *p = 0xbb;
            assert_eq!(*p, 0xbb);
            free_sized(p, big);
        }
        assert!(with_registry(|r| r.lookup(p as usize)).is_none());
    }

    /// A medium request lives in an arena and comes straight back on
    /// the next request of its class, with no mapping made or undone.
    #[test]
    fn alloc_sized_medium_routes_to_an_arena_span() {
        let size = MAX_SMALL + 1;
        let p = alloc_sized(size);
        assert!(!p.is_null());
        assert_eq!(
            with_registry(|r| r.lookup(p as usize)).map(|(k, _)| k),
            Some(RegionKind::Arena)
        );
        let meta = unsafe { (*Arena::containing(p)).meta_of(p) };
        assert_eq!(class_size(unsafe { (*meta).class() }), 5 << 10);
        let regions = with_registry(|r| r.len());
        unsafe {
            *p.add(size - 1) = 0xcc;
            free_sized(p, size);
        }
        assert_eq!(alloc_sized(5 << 10), p);
        assert_eq!(with_registry(|r| r.len()), regions);
        unsafe { free_sized(p, 5 << 10) };
        let q = alloc_sized(MAX_MEDIUM);
        assert!(!q.is_null());
        assert_eq!(
            with_registry(|r| r.lookup(q as usize)).map(|(k, _)| k),
            Some(RegionKind::Arena)
        );
        unsafe {
            *q.add(MAX_MEDIUM - 1) = 0xdd;
            free_sized(q, MAX_MEDIUM);
        }
    }

    #[test]
    fn layer0_free_recovers_a_medium_class_through_the_arena() {
        let size = 100 << 10;
        let p = alloc_sized(size);
        assert!(!p.is_null());
        unsafe { free(p) };
        assert_eq!(
            alloc_sized(size),
            p,
            "Layer 0 free did not return the block"
        );
        unsafe { free_sized(p, size) };
    }

    #[test]
    fn layer0_free_recovers_the_class_through_the_arena() {
        let p = alloc_sized(128);
        assert!(!p.is_null());
        unsafe {
            *p = 0xcd;
            free(p);
        }
        // back on the row, so the next alloc of the class is the same block
        let p2 = alloc_sized(128);
        assert_eq!(p, p2, "Layer 0 free did not return the block");
        unsafe { free_sized(p2, 128) };
    }

    #[test]
    fn layer0_free_unmaps_a_large_block() {
        let big = 3 * MAX_MEDIUM;
        let p = alloc_sized(big);
        unsafe { free(p) };
        assert!(with_registry(|r| r.lookup(p as usize)).is_none());
    }

    #[test]
    fn layer0_free_null_is_safe() {
        unsafe { free(core::ptr::null_mut()) };
    }

    #[test]
    fn layer0_free_sentinel_is_safe() {
        let s = alloc_sized(0);
        unsafe { free(s) };
    }
}

/// Allocation counters, present only under `--cfg torajs_census`.
/// Relaxed atomics: a census build measures, it does not have to be
/// the fastest build, and the counters stay right with more threads.
#[cfg(torajs_census)]
pub(crate) mod census {
    use core::sync::atomic::{AtomicU64, Ordering};

    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub static FREES: AtomicU64 = AtomicU64::new(0);
    pub static REFILLS: AtomicU64 = AtomicU64::new(0);
    pub static SPILLS: AtomicU64 = AtomicU64::new(0);
    pub static SPAN_RETIRE: AtomicU64 = AtomicU64::new(0);
    pub static MEDIUM: AtomicU64 = AtomicU64::new(0);
    pub static LARGE: AtomicU64 = AtomicU64::new(0);

    #[inline]
    pub fn alloc(size: usize) {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }

    pub fn dump() {
        use torajs_syscall::census::emit;
        emit(b"alloc.count", ALLOCS.load(Ordering::Relaxed));
        emit(b"alloc.bytes", BYTES.load(Ordering::Relaxed));
        emit(b"alloc.frees", FREES.load(Ordering::Relaxed));
        emit(b"alloc.refills", REFILLS.load(Ordering::Relaxed));
        emit(b"alloc.spills", SPILLS.load(Ordering::Relaxed));
        emit(b"alloc.span_retire", SPAN_RETIRE.load(Ordering::Relaxed));
        emit(b"alloc.medium", MEDIUM.load(Ordering::Relaxed));
        emit(b"alloc.large", LARGE.load(Ordering::Relaxed));
    }
}
