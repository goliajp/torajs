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

use crate::large::{large_alloc, large_free};
use crate::size_class::{Allocator, PER_CLASS_CAP, SIZE_CLASSES};
use crate::span::SPAN_LEN;

// ============================================================
// SpanRegistry — ptr→span O(log n) lookup
// ============================================================

/// Max spans tracked by `SpanRegistry`. = `PER_CLASS_CAP *
/// SIZE_CLASSES.len()`. Matches the upper bound the underlying
/// `size_class::Allocator` can reach. Phase 2c sharded hashmap
/// removes this cap.
pub const MAX_REGISTERED_SPANS: usize = PER_CLASS_CAP * SIZE_CLASSES.len();

#[derive(Clone, Copy)]
struct RegistryEntry {
    /// Base address of the span (mmap'd region start).
    base: usize,
    /// Size class index this span serves.
    class_idx: u8,
}

const ZERO_ENTRY: RegistryEntry = RegistryEntry {
    base: 0,
    class_idx: 0,
};

pub struct SpanRegistry {
    /// Sorted by `base` ascending. First `cur` entries occupied;
    /// remainder is zeroed but unread. Sort invariant maintained
    /// by `insert`.
    entries: [RegistryEntry; MAX_REGISTERED_SPANS],
    cur: u32,
}

impl Default for SpanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanRegistry {
    pub const fn new() -> Self {
        SpanRegistry {
            entries: [ZERO_ENTRY; MAX_REGISTERED_SPANS],
            cur: 0,
        }
    }

    /// Insert a new span entry. Maintains sorted-by-base invariant
    /// via insertion sort. O(n) but called only on span grow (rare
    /// — amortized cost negligible per-alloc).
    /// Returns `false` if the registry is at cap.
    pub fn insert(&mut self, base: usize, class_idx: u8) -> bool {
        let cur = self.cur as usize;
        if cur >= MAX_REGISTERED_SPANS {
            return false;
        }
        // Binary search for insertion point in [0, cur).
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base < base {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let insert_at = lo;
        // Shift entries [insert_at..cur] right by 1.
        let mut i = cur;
        while i > insert_at {
            self.entries[i] = self.entries[i - 1];
            i -= 1;
        }
        self.entries[insert_at] = RegistryEntry { base, class_idx };
        self.cur += 1;
        true
    }

    /// Lookup `ptr` → `class_idx`. O(log n_spans) via binary
    /// search on sorted-by-base entries.
    ///
    /// Returns `None` if `ptr` falls outside any registered span
    /// (= ptr is from `large_alloc` or is not from this allocator).
    pub fn lookup(&self, ptr: usize) -> Option<u8> {
        let cur = self.cur as usize;
        if cur == 0 {
            return None;
        }
        // Find largest i in [0, cur) where entries[i].base <= ptr.
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entries[mid].base <= ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let entry = &self.entries[lo - 1];
        if ptr >= entry.base && ptr < entry.base + SPAN_LEN {
            Some(entry.class_idx)
        } else {
            None
        }
    }

    /// Current span population count.
    #[inline]
    pub fn len(&self) -> usize {
        self.cur as usize
    }

    /// True iff no spans registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cur == 0
    }
}

// ============================================================
// Global core allocator — owns Allocator + SpanRegistry pair
// ============================================================

static CORE_LOCK: AtomicBool = AtomicBool::new(false);
static mut CORE_ALLOC: Allocator = Allocator::new();
static mut CORE_REGISTRY: SpanRegistry = SpanRegistry::new();

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
pub fn alloc_sized(size: usize) -> *mut u8 {
    if size == 0 {
        return zero_sentinel();
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        return large_alloc(size).unwrap_or(core::ptr::null_mut());
    }
    lock();
    // Detect span-grow via mapped_bytes delta — if it changed,
    // a new span was added; register its (base, class_idx) so
    // future Layer 0 free(ptr) can dispatch.
    let before_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    let p = unsafe { (*&raw mut CORE_ALLOC).alloc(size) }.unwrap_or(core::ptr::null_mut());
    let after_mapped = unsafe { (*&raw const CORE_ALLOC).mapped_bytes() };
    if !p.is_null() && after_mapped > before_mapped {
        // Span base = ptr rounded down to SPAN_LEN boundary.
        let span_base = (p as usize) & !(SPAN_LEN - 1);
        let class_idx = Allocator::bucket_for(size).unwrap_or(0) as u8;
        unsafe {
            (*&raw mut CORE_REGISTRY).insert(span_base, class_idx);
        }
    }
    unlock();
    p
}

/// Layer 1 free — caller provides original size. Skips registry
/// lookup entirely (fastest path).
///
/// # Safety
///
/// `ptr` must be a pointer returned by `alloc` / `alloc_sized`
/// with the matching `size`, not already freed.
pub unsafe fn free_sized(ptr: *mut u8, size: usize) {
    if ptr.is_null() || ptr == zero_sentinel() || size == 0 {
        return;
    }
    if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
        let _ = unsafe { large_free(ptr, size) };
        return;
    }
    lock();
    unsafe {
        (*&raw mut CORE_ALLOC).dealloc(ptr, size);
    }
    unlock();
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
    let class_idx = unsafe { (*&raw const CORE_REGISTRY).lookup(ptr as usize) };
    unlock();
    match class_idx {
        Some(idx) => {
            let size = SIZE_CLASSES[idx as usize];
            unsafe { free_sized(ptr, size) };
        }
        None => {
            // ptr not in any small-span registry — must be a
            // large_alloc result. Phase 2d will extend the
            // registry to cover LARGE-class spans; until then,
            // size-less free of a large alloc leaks.
            // This is documented dead code for the current scope.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SpanRegistry direct tests (no global state) ---

    #[test]
    fn registry_lookup_empty_is_none() {
        let r = SpanRegistry::new();
        assert!(r.lookup(0x1000).is_none());
        assert!(r.is_empty());
    }

    #[test]
    fn registry_insert_then_lookup_in_range() {
        let mut r = SpanRegistry::new();
        let base = 0x1_0000_0000usize;
        assert!(r.insert(base, 3));
        // Inside span
        assert_eq!(r.lookup(base), Some(3));
        assert_eq!(r.lookup(base + SPAN_LEN / 2), Some(3));
        assert_eq!(r.lookup(base + SPAN_LEN - 1), Some(3));
        // Outside span
        assert_eq!(r.lookup(base - 1), None);
        assert_eq!(r.lookup(base + SPAN_LEN), None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn registry_insert_maintains_sorted_invariant() {
        let mut r = SpanRegistry::new();
        // Insert in reverse-base order; lookups should still work.
        let bases = [
            0x9_0000_0000usize,
            0x3_0000_0000,
            0x7_0000_0000,
            0x1_0000_0000,
            0x5_0000_0000,
        ];
        for (i, b) in bases.iter().enumerate() {
            assert!(r.insert(*b, i as u8));
        }
        for (i, b) in bases.iter().enumerate() {
            assert_eq!(r.lookup(*b), Some(i as u8));
            assert_eq!(r.lookup(*b + SPAN_LEN / 2), Some(i as u8));
        }
        // Lookup between spans returns None.
        assert_eq!(r.lookup(0x2_0000_0000), None);
        assert_eq!(r.lookup(0x4_0000_0000), None);
    }

    #[test]
    fn registry_lookup_below_lowest_is_none() {
        let mut r = SpanRegistry::new();
        r.insert(0x5_0000_0000, 1);
        assert!(r.lookup(0x1_0000_0000).is_none());
    }

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
