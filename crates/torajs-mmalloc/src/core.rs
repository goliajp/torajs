//! Core API — Layer 0 / Layer 1 metal-tier allocator surface.
//!
//! Phase 2a items 3+4 of the metal-tier redesign per
//! `docs/v0.7-A2-finding.md`. Provides:
//!
//! - **Layer 0** — `alloc(size)` / `free(ptr)`. `free(ptr)`
//!   consults `SpanRegistry` for ptr→class lookup; matches libc-
//!   shape contract WITHOUT per-alloc SHIM_HEADER. The ptr→span
//!   info lives in span metadata (one entry per span, not one per
//!   alloc) — header overhead amortizes by `slot_count`.
//! - **Layer 1** — `alloc_sized(size)` / `free_sized(ptr, size)`.
//!   Caller-knows-size fast path; skips `SpanRegistry` lookup
//!   entirely.
//!
//! Both layers share the underlying `size_class::Allocator`
//! (Phase 2a item 2 span-backed shape). Layer 0 free is the only
//! path that pays the lookup cost; sub-crate hot paths will use
//! Layer 1 once IR codegen migrates (Phase 2e).
//!
//! Phase 2a item 5 will migrate `extern_api`'s `__torajs_malloc` /
//! `__torajs_free` to wrap these layers; `__torajs_libc_*` shim
//! becomes Layer 2 wrapping Layer 1 (SHIM_HEADER retained only in
//! Layer 2 for external C consumers whose API truly lost size).
//!
//! Phase 2c will upgrade `SpanRegistry` to a per-CPU sharded
//! hashmap with O(1) lookup; the current binary-search form is
//! O(log n) — already orders better than the size_class fallback
//! O(n) scan path and adequate for Phase 2a/2b workloads.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::central::CentralQueue;
use crate::large::{large_alloc, large_free};
use crate::size_class::{Allocator, SIZE_CLASSES};
use crate::span::SPAN_LEN;
use crate::span_registry::{LARGE_CLASS_IDX, SpanRegistry};
use crate::tlab::{TLAB_CACHE_DEPTH, TlabCache};

// ============================================================
// Global core allocator — owns Allocator + SpanRegistry pair
// ============================================================

static CORE_LOCK: AtomicBool = AtomicBool::new(false);
static mut CORE_ALLOC: Allocator = Allocator::new();
static mut CORE_REGISTRY: SpanRegistry = SpanRegistry::new();
// Step 16-c-2 (2026-05-29): downgraded from `#[thread_local]` to a
// plain `static mut` to drop the last `__tlv_bootstrap` undefined
// symbol from user binaries (A5 zero-libc-undef goal). On macOS
// aarch64 `#[thread_local]` forces a `$tlv$init` / `__tlv_bootstrap`
// dyld dependency — see docs/v0.7-A5-finding.md. The single-threaded
// runtime has no concurrent observer, so a process-wide TLAB is sound.
//
// Access via `&raw mut` like CORE_ALLOC / CORE_REGISTRY above (clears
// the edition-2024 `static_mut_refs` lint). `TlabCache::new()` is
// const — the static initializes at compile time, no ctor.
//
// MULTI-THREAD RE-DERIVATION (v0.8 backlog): a process-wide TLAB
// defeats the per-thread isolation a threaded runtime needs. When the
// first threaded path lands (Promise/async/worker), re-derive per-
// thread TLABs via a syscall-thread-id-indexed manual array (NOT
// `#[thread_local]` — Darwin local-exec TLS still routes via tlv).
//
// `#[unsafe(no_mangle)] pub` (Phase 2e item 13): stable symbol name
// so the toolchain can inline TLAB.pop/push at alloc/free sites
// (LLVM-era backend did; the native ARM64 re-port is swap-3+
// backlog — see cmd_build's synthesize_obj_alloc).
#[unsafe(no_mangle)]
pub static mut __torajs_core_tlab: TlabCache = TlabCache::new();

/// Process-wide central queue (Phase 2c item 10). Lock-free MPMC
/// stack per size class; acts as the TLAB overflow buffer + cross-
/// thread free landing zone. Single-thread runtime today: TLAB
/// overflow → Central.push (lock-free, faster than Allocator.dealloc's
/// O(n_spans) scan); alloc TLAB.miss → drain Central back to TLAB
/// before falling through to Allocator.alloc. Multi-thread future:
/// foreign-thread free → Central.push automatically routes to
/// owning thread's next refill cycle.
static CORE_CENTRAL: CentralQueue = CentralQueue::new();

#[inline]
fn lock() {
    while CORE_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
fn unlock() {
    CORE_LOCK.store(false, Ordering::Release);
}

/// Sentinel returned on zero-size alloc to keep callers from
/// confusing with NULL=OOM. Matches glibc behavior.
#[inline]
fn zero_sentinel() -> *mut u8 {
    &raw const CORE_LOCK as *mut u8
}

// ============================================================
// Layer 1 — alloc_sized / free_sized (hot path, no lookup)
// ============================================================

/// Layer 1 alloc — caller knows size. Hot path; `free_sized`
/// skips registry. Returns NULL on OOM, sentinel on `size == 0`.
///
/// TLAB fast path (Phase 2b item 7): if a slot of the right size
/// class is cached in __torajs_core_tlab, return it directly (~3 cycles
/// inside the locked section). Miss falls through to
/// `CORE_ALLOC.alloc` (size_class span freelist).
///
/// `#[inline(always)]` (Phase 2e item 13a): lets fat LTO + cc -flto
/// inline the hot path into user-binary IR, eliminating extern "C"
/// call overhead per alloc.
#[inline(always)]
pub fn alloc_sized(size: usize) -> *mut u8 {
    if size == 0 {
        return zero_sentinel();
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        // Large path — direct mmap + registry insert so Layer 0
        // free(ptr) can recover size for `large_free` dispatch.
        let p = match large_alloc(size) {
            Ok(p) => p,
            Err(_) => return core::ptr::null_mut(),
        };
        // large_alloc rounds size up to PAGE_4K internally; mirror
        // here so the registered size matches the mmap'd region's
        // actual length (needed for ptr-containment lookup).
        let rounded = (size.max(1) + 4095) & !4095;
        lock();
        unsafe { (*&raw mut CORE_REGISTRY).insert(p as usize, LARGE_CLASS_IDX, rounded) };
        unlock();
        return p;
    }
    let class_idx = match Allocator::bucket_for(size) {
        Some(i) => i,
        None => return core::ptr::null_mut(),
    };
    // TLAB fast path — plain global, single direct load + ~3 cyc pop.
    // Common case under hot loops.
    let tlab = &raw mut __torajs_core_tlab;
    if let Some(p) = unsafe { (*tlab).pop(class_idx) } {
        return p;
    }
    // TLAB miss — drain up to TLAB_CACHE_DEPTH slots from Central
    // back into per-thread TLAB. Central is lock-free MPMC.
    let first_central = CORE_CENTRAL.pop(class_idx);
    if let Some(p) = first_central {
        for _ in 1..TLAB_CACHE_DEPTH {
            match CORE_CENTRAL.pop(class_idx) {
                Some(q) => {
                    if !unsafe { (*tlab).push(class_idx, q) } {
                        // TLAB filled mid-drain — push leftover
                        // back to Central for next round.
                        unsafe { CORE_CENTRAL.push(class_idx, q) };
                        break;
                    }
                }
                None => break,
            }
        }
        return p;
    }
    // TLAB + Central both empty — central Allocator (under lock,
    // since Allocator's per-class span lists need sync). Only
    // entered on first allocs / cold paths.
    lock();
    let before_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    let p = unsafe { (*&raw mut CORE_ALLOC).alloc(size) }.unwrap_or(core::ptr::null_mut());
    let after_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    if !p.is_null() && after_mapped > before_mapped {
        // Span base = ptr rounded down to SPAN_LEN boundary.
        let span_base = (p as usize) & !(SPAN_LEN - 1);
        unsafe {
            (*&raw mut CORE_REGISTRY).insert(span_base, class_idx as u8, SPAN_LEN);
        }
    }
    unlock();
    p
}

/// Layer 1 free — caller provides original size. Skips registry
/// lookup entirely (fastest path).
///
/// TLAB fast path (Phase 2b item 7): push the freed slot into
/// __torajs_core_tlab so the next `alloc_sized` of the same size can hand
/// it back without touching the central span freelist (~3 cycles
/// inside the locked section). If the TLAB is full for this
/// class, fall through to central `Allocator.dealloc`.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized`
/// with the matching `size`, not already freed.
#[inline(always)]
pub unsafe fn free_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr == zero_sentinel() || size == 0 {
        return;
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        // Large path — deregister from registry then munmap.
        lock();
        unsafe { (*&raw mut CORE_REGISTRY).remove(ptr as usize) };
        unlock();
        let _ = unsafe { large_free(ptr, size) };
        return;
    }
    let class_idx = match Allocator::bucket_for(size) {
        Some(i) => i,
        None => return,
    };
    // TLAB fast path — plain global, single direct load + ~3 cyc push.
    let tlab = &raw mut __torajs_core_tlab;
    if unsafe { (*tlab).push(class_idx, ptr) } {
        return;
    }
    // TLAB full — push to Central (lock-free MPMC). Next alloc-
    // miss on this class drains Central back to TLAB amortized.
    unsafe { CORE_CENTRAL.push(class_idx, ptr) };
}

// ============================================================
// Layer 0 — alloc / free (size recovered from registry)
// ============================================================

/// Layer 0 alloc — same shape as `alloc_sized` (size is always
/// known by the caller in any sane API). Kept as a distinct symbol
/// for surface-symmetry with `free` (which does need registry).
#[inline]
pub fn alloc(size: usize) -> *mut u8 {
    alloc_sized(size)
}

/// Layer 0 free — caller has no size. SpanRegistry lookup
/// recovers size class. O(log n_spans) per free.
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized`,
/// not already freed.
pub unsafe fn free(ptr: *mut u8) {
    if ptr.is_null() || ptr == zero_sentinel() {
        return;
    }
    lock();
    let lookup_result = unsafe { (*&raw const CORE_REGISTRY).lookup(ptr as usize) };
    unlock();
    match lookup_result {
        Some((LARGE_CLASS_IDX, large_size)) => {
            // Large alloc — deregister then munmap.
            lock();
            unsafe { (*&raw mut CORE_REGISTRY).remove(ptr as usize) };
            unlock();
            let _ = unsafe { large_free(ptr, large_size) };
        }
        Some((idx, _)) => {
            // Small span — recover size from class.
            let size = SIZE_CLASSES[idx as usize];
            unsafe { free_sized(ptr, size) };
        }
        None => {
            // ptr not in any registered region — was not allocated
            // by this allocator (or already-freed). No-op (matches
            // libc free(NULL) safety contract).
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // --- Layer 1 alloc_sized / free_sized round-trip ---

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
        assert!(
            !p.is_null(),
            "zero-size alloc returned null (expected sentinel)"
        );
        // Free of sentinel must be a no-op (not corrupt).
        unsafe { free_sized(p, 0) };
    }

    #[test]
    fn alloc_sized_large_routes_to_large_alloc() {
        // size > biggest size class → large_alloc path.
        let big = SIZE_CLASSES[SIZE_CLASSES.len() - 1] + 1;
        let p = alloc_sized(big);
        assert!(!p.is_null());
        unsafe {
            // Touch first byte; mmap'd region should be writable.
            *p = 0xbb;
            assert_eq!(*p, 0xbb);
            free_sized(p, big);
        }
    }

    // --- Layer 0 free (registry lookup) ---

    #[test]
    fn layer0_free_recovers_size_via_registry() {
        // Layer 1 alloc → Layer 0 free. Registry should have been
        // populated by alloc_sized's grow hook.
        let p = alloc_sized(128);
        assert!(!p.is_null());
        unsafe {
            *p = 0xcd;
            free(p);
        }
        // Subsequent alloc of same size should reuse the freed
        // slot (Span freelist is LIFO).
        let p2 = alloc_sized(128);
        assert_eq!(p, p2, "Layer 0 free didn't return slot to span");
        unsafe { free_sized(p2, 128) };
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
