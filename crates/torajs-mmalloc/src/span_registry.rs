//! SpanRegistry — ptr→region O(log n) lookup table for Layer 0
//! `free(ptr)` dispatch (small-span class recovery + large-alloc
//! size recovery). Extracted from `core.rs` (chunk 633 file-size
//! split when the registry went mmap-backed growable).

use crate::large::page_round_up;
use torajs_syscall::{mmap_anon_rw, munmap};

/// First registry growth — 128 entries × 24 B fits one 4 KB page.
const REGISTRY_INITIAL_CAP: usize = 128;

/// Sentinel class index marking a large (mmap-direct) allocation
/// rather than a small-span slot. Phase 2d item 11+12.
pub const LARGE_CLASS_IDX: u8 = u8::MAX;

#[derive(Clone, Copy)]
struct RegistryEntry {
    /// Base address of the registered region (mmap'd start).
    base: usize,
    /// Size class index — 0..SIZE_CLASSES.len() for small span,
    /// `LARGE_CLASS_IDX` for large mmap-direct allocations.
    class_idx: u8,
    /// Region size in bytes — used for ptr-containment check
    /// (small span: SPAN_LEN; large alloc: PAGE_4K-rounded size).
    /// Carrying it per-entry lets `lookup` and `remove` uniformly
    /// handle both small + large without branching on class_idx.
    size: usize,
}

/// Growable registry array, mmap-backed (chunk 633 — the former
/// `[RegistryEntry; PER_CLASS_CAP × 9]` static array capped out
/// together with the allocator's span arrays; see
/// `size_class::SpanVec`). `RegistryEntry` is `Copy` with no drop
/// glue, so growth is a plain byte copy + munmap of the old
/// backing.
pub struct SpanRegistry {
    /// Sorted by `base` ascending. First `cur` entries occupied.
    /// Sort invariant maintained by `insert`.
    entries: *mut RegistryEntry,
    cap: usize,
    cur: usize,
}

impl Default for SpanRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SpanRegistry {
    fn drop(&mut self) {
        // Test-process hygiene only — the production instance is a
        // never-dropped `static mut`.
        if !self.entries.is_null() {
            let _ = unsafe {
                munmap(
                    self.entries as *mut u8,
                    page_round_up(self.cap * core::mem::size_of::<RegistryEntry>()),
                )
            };
        }
    }
}

impl SpanRegistry {
    pub const fn new() -> Self {
        SpanRegistry {
            entries: core::ptr::null_mut(),
            cap: 0,
            cur: 0,
        }
    }

    #[inline]
    fn entry(&self, i: usize) -> &RegistryEntry {
        debug_assert!(i < self.cur);
        unsafe { &*self.entries.add(i) }
    }

    /// Double the mmap'd backing. `false` on kernel mmap failure.
    fn grow(&mut self) -> bool {
        let new_cap = if self.cap == 0 {
            REGISTRY_INITIAL_CAP
        } else {
            self.cap * 2
        };
        let bytes = page_round_up(new_cap * core::mem::size_of::<RegistryEntry>());
        let Ok(p) = mmap_anon_rw(bytes) else {
            return false;
        };
        let new_entries = p as *mut RegistryEntry;
        if self.cur > 0 {
            unsafe { core::ptr::copy_nonoverlapping(self.entries, new_entries, self.cur) };
        }
        if !self.entries.is_null() {
            let _ = unsafe {
                munmap(
                    self.entries as *mut u8,
                    page_round_up(self.cap * core::mem::size_of::<RegistryEntry>()),
                )
            };
        }
        self.entries = new_entries;
        self.cap = new_cap;
        true
    }

    /// Insert a new region entry. Maintains sorted-by-base
    /// invariant via insertion sort. O(n) but called only on
    /// span grow or large alloc (both rare — amortized cost
    /// negligible per-alloc).
    /// Returns `false` on kernel mmap failure while growing.
    pub fn insert(&mut self, base: usize, class_idx: u8, size: usize) -> bool {
        let cur = self.cur;
        if cur >= self.cap && !self.grow() {
            return false;
        }
        // Binary search for insertion point in [0, cur).
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entry(mid).base < base {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let insert_at = lo;
        // Shift entries [insert_at..cur) right by 1.
        if cur > insert_at {
            unsafe {
                core::ptr::copy(
                    self.entries.add(insert_at),
                    self.entries.add(insert_at + 1),
                    cur - insert_at,
                );
            }
        }
        unsafe {
            core::ptr::write(
                self.entries.add(insert_at),
                RegistryEntry {
                    base,
                    class_idx,
                    size,
                },
            );
        }
        self.cur += 1;
        true
    }

    /// Lookup `ptr` → `(class_idx, size)`. O(log n) via binary
    /// search on sorted-by-base entries.
    ///
    /// Returns `None` if `ptr` falls outside any registered region.
    /// For Phase 2d large-alloc dispatch: returned `class_idx` is
    /// `LARGE_CLASS_IDX` for mmap-direct large allocs; `size` is
    /// then the mmap'd size to pass to `large_free`.
    pub fn lookup(&self, ptr: usize) -> Option<(u8, usize)> {
        let cur = self.cur;
        if cur == 0 {
            return None;
        }
        // Find largest i in [0, cur) where entries[i].base <= ptr.
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entry(mid).base <= ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let entry = self.entry(lo - 1);
        if ptr >= entry.base && ptr < entry.base + entry.size {
            Some((entry.class_idx, entry.size))
        } else {
            None
        }
    }

    /// Remove the entry whose region contains `ptr`. O(log n +
    /// shift). Returns `Some((class_idx, size))` of the removed
    /// region, or `None` if `ptr` is not in any registered region.
    /// Used by Phase 2d large-alloc free path to deregister
    /// before `large_free`'s munmap.
    pub fn remove(&mut self, ptr: usize) -> Option<(u8, usize)> {
        let cur = self.cur;
        if cur == 0 {
            return None;
        }
        // Binary search for containing entry.
        let mut lo = 0usize;
        let mut hi = cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entry(mid).base <= ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let idx = lo - 1;
        let entry = *self.entry(idx);
        if ptr < entry.base || ptr >= entry.base + entry.size {
            return None;
        }
        // Shift [idx+1..cur) left by 1.
        if cur - 1 > idx {
            unsafe {
                core::ptr::copy(
                    self.entries.add(idx + 1),
                    self.entries.add(idx),
                    cur - 1 - idx,
                );
            }
        }
        self.cur -= 1;
        Some((entry.class_idx, entry.size))
    }

    /// Current span population count.
    #[inline]
    pub fn len(&self) -> usize {
        self.cur
    }

    /// True iff no spans registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cur == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::SPAN_LEN;

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
        assert!(r.insert(base, 3, SPAN_LEN));
        // Inside span
        assert_eq!(r.lookup(base), Some((3, SPAN_LEN)));
        assert_eq!(r.lookup(base + SPAN_LEN / 2), Some((3, SPAN_LEN)));
        assert_eq!(r.lookup(base + SPAN_LEN - 1), Some((3, SPAN_LEN)));
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
            assert!(r.insert(*b, i as u8, SPAN_LEN));
        }
        for (i, b) in bases.iter().enumerate() {
            assert_eq!(r.lookup(*b), Some((i as u8, SPAN_LEN)));
            assert_eq!(r.lookup(*b + SPAN_LEN / 2), Some((i as u8, SPAN_LEN)));
        }
        // Lookup between spans returns None.
        assert_eq!(r.lookup(0x2_0000_0000), None);
        assert_eq!(r.lookup(0x4_0000_0000), None);
    }

    #[test]
    fn registry_lookup_below_lowest_is_none() {
        let mut r = SpanRegistry::new();
        r.insert(0x5_0000_0000, 1, SPAN_LEN);
        assert!(r.lookup(0x1_0000_0000).is_none());
    }

    #[test]
    fn registry_remove_drops_entry() {
        let mut r = SpanRegistry::new();
        let bases = [0x1_0000_0000usize, 0x3_0000_0000, 0x5_0000_0000];
        for (i, b) in bases.iter().enumerate() {
            assert!(r.insert(*b, i as u8, SPAN_LEN));
        }
        assert_eq!(r.len(), 3);
        // Remove middle entry.
        let (class_idx, size) = r.remove(0x3_0000_0000 + 100).expect("remove middle");
        assert_eq!(class_idx, 1);
        assert_eq!(size, SPAN_LEN);
        assert_eq!(r.len(), 2);
        // First and last still accessible.
        assert_eq!(r.lookup(0x1_0000_0000), Some((0, SPAN_LEN)));
        assert_eq!(r.lookup(0x5_0000_0000), Some((2, SPAN_LEN)));
        // Removed range lookup returns None.
        assert!(r.lookup(0x3_0000_0000 + 100).is_none());
    }

    #[test]
    fn registry_large_class_tracked() {
        // Phase 2d: LARGE_CLASS_IDX entries with custom size.
        let mut r = SpanRegistry::new();
        let large_base = 0x10_0000_0000usize;
        let large_size = 256 * 1024; // 256 KB large alloc
        assert!(r.insert(large_base, LARGE_CLASS_IDX, large_size));
        assert_eq!(r.lookup(large_base), Some((LARGE_CLASS_IDX, large_size)));
        assert_eq!(
            r.lookup(large_base + large_size - 1),
            Some((LARGE_CLASS_IDX, large_size))
        );
        // Just outside the large region.
        assert_eq!(r.lookup(large_base + large_size), None);
    }
}
