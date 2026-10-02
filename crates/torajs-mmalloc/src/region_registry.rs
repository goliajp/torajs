//! Region registry — the sorted table of every mapping the allocator
//! handed blocks out of: arenas and large allocations. `free(ptr)`
//! with no size finds its region here by binary search; an arena then
//! names the block's span itself.
//!
//! One global instance behind a spin lock; the lock guards nothing
//! else. Entries come and go once per arena or per large allocation,
//! so the table stays small and insertion stays cheap.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::large::page_round_up;
use torajs_syscall::{mmap_anon_rw, munmap};

/// First growth: 128 entries × 24 B fits one 4 KB page.
const REGISTRY_INITIAL_CAP: usize = 128;

/// What kind of mapping a region is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// An arena of spans; the block's class is in the arena header.
    Arena,
    /// One large allocation, `size` bytes mapped for it alone.
    Large,
}

#[derive(Clone, Copy)]
struct RegionEntry {
    base: usize,
    size: usize,
    kind: RegionKind,
}

/// Growable, mmap-backed, sorted by `base`. `RegionEntry` is `Copy`
/// with no drop glue, so growth is a byte copy and a munmap.
pub struct RegionRegistry {
    entries: *mut RegionEntry,
    cap: usize,
    cur: usize,
}

static LOCK: AtomicBool = AtomicBool::new(false);
static mut REGISTRY: RegionRegistry = RegionRegistry::new();

/// Run `f` on the global registry under its lock.
pub(crate) fn with_registry<R>(f: impl FnOnce(&mut RegionRegistry) -> R) -> R {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    let registry: *mut RegionRegistry = &raw mut REGISTRY;
    let r = f(unsafe { &mut *registry });
    LOCK.store(false, Ordering::Release);
    r
}

/// A non-null address no block can have; the zero-size sentinel.
#[inline]
pub(crate) fn lock_addr() -> *mut u8 {
    &raw const LOCK as *mut u8
}

impl Default for RegionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RegionRegistry {
    fn drop(&mut self) {
        // test-process hygiene only; the global never drops
        if !self.entries.is_null() {
            let _ = unsafe {
                munmap(
                    self.entries as *mut u8,
                    page_round_up(self.cap * size_of::<RegionEntry>()),
                )
            };
        }
    }
}

impl RegionRegistry {
    pub const fn new() -> Self {
        RegionRegistry {
            entries: core::ptr::null_mut(),
            cap: 0,
            cur: 0,
        }
    }

    #[inline]
    fn entry(&self, i: usize) -> &RegionEntry {
        debug_assert!(i < self.cur);
        unsafe { &*self.entries.add(i) }
    }

    /// Double the backing. `false` when the kernel refuses.
    fn grow(&mut self) -> bool {
        let new_cap = if self.cap == 0 {
            REGISTRY_INITIAL_CAP
        } else {
            self.cap * 2
        };
        let bytes = page_round_up(new_cap * size_of::<RegionEntry>());
        let Ok(p) = mmap_anon_rw(bytes) else {
            return false;
        };
        let new_entries = p as *mut RegionEntry;
        if self.cur > 0 {
            unsafe { core::ptr::copy_nonoverlapping(self.entries, new_entries, self.cur) };
        }
        if !self.entries.is_null() {
            let _ = unsafe {
                munmap(
                    self.entries as *mut u8,
                    page_round_up(self.cap * size_of::<RegionEntry>()),
                )
            };
        }
        self.entries = new_entries;
        self.cap = new_cap;
        true
    }

    /// Index of the first entry whose base is `>= ptr`.
    fn lower_bound(&self, ptr: usize) -> usize {
        let mut lo = 0usize;
        let mut hi = self.cur;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.entry(mid).base < ptr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Index of the entry whose region holds `ptr`.
    fn find(&self, ptr: usize) -> Option<usize> {
        let at = self.lower_bound(ptr + 1);
        if at == 0 {
            return None;
        }
        let e = self.entry(at - 1);
        (ptr >= e.base && ptr < e.base + e.size).then_some(at - 1)
    }

    /// Register a region. `false` when growing the table failed.
    pub fn insert(&mut self, base: usize, size: usize, kind: RegionKind) -> bool {
        let cur = self.cur;
        if cur >= self.cap && !self.grow() {
            return false;
        }
        let at = self.lower_bound(base);
        if cur > at {
            unsafe { core::ptr::copy(self.entries.add(at), self.entries.add(at + 1), cur - at) };
        }
        unsafe { core::ptr::write(self.entries.add(at), RegionEntry { base, size, kind }) };
        self.cur += 1;
        true
    }

    /// The region holding `ptr`, as `(kind, size)`.
    pub fn lookup(&self, ptr: usize) -> Option<(RegionKind, usize)> {
        let e = self.entry(self.find(ptr)?);
        Some((e.kind, e.size))
    }

    /// Drop the region holding `ptr`; returns its `(kind, size)`.
    pub fn remove(&mut self, ptr: usize) -> Option<(RegionKind, usize)> {
        let idx = self.find(ptr)?;
        let e = *self.entry(idx);
        let cur = self.cur;
        if cur - 1 > idx {
            unsafe {
                core::ptr::copy(
                    self.entries.add(idx + 1),
                    self.entries.add(idx),
                    cur - 1 - idx,
                )
            };
        }
        self.cur -= 1;
        Some((e.kind, e.size))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.cur
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.cur == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ARENA_LEN;

    #[test]
    fn lookup_on_empty_is_none() {
        let r = RegionRegistry::new();
        assert!(r.lookup(0x1000).is_none());
        assert!(r.is_empty());
    }

    #[test]
    fn insert_then_lookup_in_range() {
        let mut r = RegionRegistry::new();
        let base = 0x1_0000_0000usize;
        assert!(r.insert(base, ARENA_LEN, RegionKind::Arena));
        assert_eq!(r.lookup(base), Some((RegionKind::Arena, ARENA_LEN)));
        assert_eq!(
            r.lookup(base + ARENA_LEN / 2),
            Some((RegionKind::Arena, ARENA_LEN))
        );
        assert_eq!(
            r.lookup(base + ARENA_LEN - 1),
            Some((RegionKind::Arena, ARENA_LEN))
        );
        assert_eq!(r.lookup(base - 1), None);
        assert_eq!(r.lookup(base + ARENA_LEN), None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn insert_keeps_the_table_sorted() {
        let mut r = RegionRegistry::new();
        let bases = [
            0x9_0000_0000usize,
            0x3_0000_0000,
            0x7_0000_0000,
            0x1_0000_0000,
            0x5_0000_0000,
        ];
        for b in bases {
            assert!(r.insert(b, ARENA_LEN, RegionKind::Arena));
        }
        for b in bases {
            assert_eq!(r.lookup(b + 100), Some((RegionKind::Arena, ARENA_LEN)));
        }
        assert_eq!(r.lookup(0x2_0000_0000), None);
        assert_eq!(r.lookup(0x4_0000_0000), None);
    }

    #[test]
    fn remove_drops_only_its_entry() {
        let mut r = RegionRegistry::new();
        let bases = [0x1_0000_0000usize, 0x3_0000_0000, 0x5_0000_0000];
        for b in bases {
            assert!(r.insert(b, ARENA_LEN, RegionKind::Arena));
        }
        assert_eq!(
            r.remove(0x3_0000_0000 + 100),
            Some((RegionKind::Arena, ARENA_LEN))
        );
        assert_eq!(r.len(), 2);
        assert!(r.lookup(0x3_0000_0000 + 100).is_none());
        assert!(r.lookup(0x1_0000_0000).is_some());
        assert!(r.lookup(0x5_0000_0000).is_some());
        assert_eq!(r.remove(0x4_0000_0000), None);
    }

    #[test]
    fn large_regions_carry_their_own_size() {
        let mut r = RegionRegistry::new();
        let base = 0x10_0000_0000usize;
        let size = 256 * 1024;
        assert!(r.insert(base, size, RegionKind::Large));
        assert_eq!(r.lookup(base), Some((RegionKind::Large, size)));
        assert_eq!(r.lookup(base + size - 1), Some((RegionKind::Large, size)));
        assert_eq!(r.lookup(base + size), None);
    }

    #[test]
    fn grows_past_the_first_page() {
        let mut r = RegionRegistry::new();
        for i in 0..(REGISTRY_INITIAL_CAP * 3) {
            assert!(r.insert(i * ARENA_LEN + ARENA_LEN, ARENA_LEN, RegionKind::Arena));
        }
        assert_eq!(r.len(), REGISTRY_INITIAL_CAP * 3);
        assert_eq!(
            r.lookup(200 * ARENA_LEN + 5),
            Some((RegionKind::Arena, ARENA_LEN))
        );
    }
}
