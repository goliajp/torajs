//! Size-class free-list allocator built on top of [`PageBump`].
//!
//! Power-of-two buckets (16/32/64/128/256/512/1024/2048/4096).
//! Within each bucket: a LIFO free-list of recycled blocks; the
//! head is stored as a `NonNull<FreeListNode>` rooted in the
//! `Allocator` struct's `buckets` array.
//!
//! Allocations larger than 4096 bytes go straight through to a
//! fresh `mmap` (page-aligned region) — see [`super::large`].
//!
//! The allocator is intentionally a struct (not a global) at this
//! layer. The extern "C" `__torajs_malloc` / `__torajs_free`
//! surface (v0.7-A2 step 5) wraps a global instance behind a
//! Mutex.

use core::mem::size_of;
use core::ptr::{self, NonNull};

use crate::page::{PAGE_SIZE, PageBump};

/// Power-of-two size classes covered by the free-list. Requests
/// larger than the last entry route to large_alloc.
pub const SIZE_CLASSES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Max active pages held by an `Allocator`. 1024 × 16 KB = 16 MB of
/// addressable small-allocation arena. Larger user-binary memory
/// budgets need bumping this OR rolling a metadata-page scheme.
pub const MAX_PAGES: usize = 1024;

#[repr(C)]
struct FreeListNode {
    next: Option<NonNull<FreeListNode>>,
}

pub struct Allocator {
    /// Bounded ring of mmap'd pages, lazily populated. `pages[i] =
    /// None` means slot `i` hasn't been allocated yet.
    pages: [Option<PageBump>; MAX_PAGES],
    /// Cursor into `pages` — next slot to populate.
    cur: usize,
    /// LIFO free-list head per size class.
    buckets: [Option<NonNull<FreeListNode>>; SIZE_CLASSES.len()],
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub const fn new() -> Self {
        const NONE_PAGE: Option<PageBump> = None;
        Allocator {
            pages: [NONE_PAGE; MAX_PAGES],
            cur: 0,
            buckets: [None; SIZE_CLASSES.len()],
        }
    }

    /// Round `size` up to the next size class; returns `None` if
    /// `size` exceeds the largest bucket.
    pub fn bucket_for(size: usize) -> Option<usize> {
        if size == 0 {
            return Some(0);
        }
        SIZE_CLASSES.iter().position(|&c| size <= c)
    }

    /// Allocate `size` bytes from the appropriate size-class
    /// bucket. Returns `None` on OOM (too many pages allocated, or
    /// kernel mmap failure). `size` past 4096 returns `None` —
    /// caller should route to large_alloc.
    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        let bucket = Self::bucket_for(size)?;
        let class_size = SIZE_CLASSES[bucket];

        // Pop from free-list first
        if let Some(node) = self.buckets[bucket].take() {
            // SAFETY: node was put on the LIFO by a prior `free`
            // call, which only stores blocks that came from
            // alloc()/page bump. node's `next` was written when
            // it was added.
            self.buckets[bucket] = unsafe { node.as_ref().next };
            return Some(node.as_ptr() as *mut u8);
        }

        // Bump from current page, fall through to new page if full
        loop {
            if let Some(page) = self.current_page_mut() {
                if let Some(p) = page.try_bump(class_size) {
                    return Some(p);
                }
            }
            self.advance_page()?;
        }
    }

    /// Release a previously-allocated block. `size` must be the
    /// SAME value passed to `alloc` (size-class allocator has no
    /// per-block size metadata — caller bookkeeping required).
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by `alloc(size)`, and not
    /// already freed (double-free is UB and will corrupt the
    /// free-list).
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize) {
        let Some(bucket) = Self::bucket_for(size) else {
            // Out of bucket range — caller should have used
            // large_alloc/large_free; silently dropping (and
            // leaking) the request keeps the invariant simple.
            return;
        };
        let node_ptr = ptr as *mut FreeListNode;
        // Each freed block must be at least size_of::<FreeListNode>().
        // SIZE_CLASSES[0] = 16 bytes; FreeListNode is 8 bytes
        // (single Option<NonNull>). Invariant holds.
        debug_assert!(SIZE_CLASSES[bucket] >= size_of::<FreeListNode>());
        unsafe {
            ptr::write(
                node_ptr,
                FreeListNode {
                    next: self.buckets[bucket],
                },
            );
            self.buckets[bucket] = Some(NonNull::new_unchecked(node_ptr));
        }
    }

    fn current_page_mut(&mut self) -> Option<&mut PageBump> {
        if self.cur == 0 {
            return None;
        }
        self.pages[self.cur - 1].as_mut()
    }

    fn advance_page(&mut self) -> Option<()> {
        if self.cur >= MAX_PAGES {
            return None;
        }
        let page = PageBump::alloc_page().ok()?;
        self.pages[self.cur] = Some(page);
        self.cur += 1;
        Some(())
    }

    /// Total bytes allocated from the kernel (sum of mmap'd page
    /// sizes). Diagnostic, not a runtime hot-path.
    pub fn mapped_bytes(&self) -> usize {
        self.cur * PAGE_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_free_recycles() {
        let mut a = Allocator::new();
        let p1 = a.alloc(16).expect("alloc 16");
        unsafe { *p1 = 0xab };
        unsafe { a.dealloc(p1, 16) };
        // Next alloc of same bucket should hand back the same
        // block (LIFO).
        let p2 = a.alloc(16).expect("realloc 16");
        assert_eq!(p1, p2, "free list not recycling");
    }

    #[test]
    fn bucket_routing() {
        assert_eq!(Allocator::bucket_for(1), Some(0));
        assert_eq!(Allocator::bucket_for(16), Some(0));
        assert_eq!(Allocator::bucket_for(17), Some(1));
        assert_eq!(Allocator::bucket_for(4096), Some(8));
        assert_eq!(Allocator::bucket_for(4097), None);
    }

    #[test]
    fn cross_page_alloc() {
        let mut a = Allocator::new();
        // Fill page 1 with 256-class allocations: 16 KB / 256 = 64
        // blocks fits exactly; the 65th should trigger a new page.
        for _ in 0..64 {
            let p = a.alloc(256).expect("alloc 256");
            unsafe { *p = 0xcd };
        }
        let p = a.alloc(256).expect("alloc 256 across pages");
        unsafe { *p = 0xef };
        assert_eq!(a.mapped_bytes(), 2 * PAGE_SIZE);
    }

    #[test]
    fn alloc_too_large_returns_none() {
        let mut a = Allocator::new();
        assert!(
            a.alloc(8192).is_none(),
            "8192 > max bucket — caller routes to large_alloc"
        );
    }

    #[test]
    fn writable_freshly_mapped() {
        let mut a = Allocator::new();
        for size in [16, 32, 64, 128, 256, 512, 1024, 2048, 4096] {
            let p = a.alloc(size).expect("alloc");
            unsafe {
                for off in 0..size {
                    ptr::write(p.add(off), (off & 0xff) as u8);
                }
                for off in 0..size {
                    assert_eq!(*p.add(off), (off & 0xff) as u8);
                }
            }
        }
    }
}
