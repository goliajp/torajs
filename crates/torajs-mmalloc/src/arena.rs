//! Arena — a 1 MiB, 1 MiB-aligned mapping carved into 16 KiB pages,
//! where spans live.
//!
//! Page 0 is the header: the owning heap, the heap's list links, a
//! bitmap of free pages, each page's span, and the [`SpanMeta`] of
//! the span starting at each page. Any block's arena is its address
//! with the low 20 bits cleared, and its span is one table read from
//! there, so the free path needs no search.
//!
//! The kernel does not promise a 1 MiB-aligned mapping, so an arena
//! maps twice its length and unmaps the slack on both sides. When a
//! span's pages come back the bitmap takes them; when every page is
//! back the whole arena is unmapped, which is how memory returns to
//! the OS.
//!
//! Shape after mimalloc segments (header in page 0, per-page metadata
//! there) and Go's heap arenas (page bitmap).

use core::ptr;

use torajs_syscall::sysno::{MADV_FREE_REUSABLE, MADV_FREE_REUSE};
use torajs_syscall::{madvise, mmap_anon_rw, munmap};

use crate::size_class::{slots_per_span, span_pages};
use crate::span::SpanMeta;
use crate::tlab::TlabCache;

/// Arena length and alignment.
pub const ARENA_LEN: usize = 1 << 20;
/// Page size inside an arena; the kernel's own page on this target.
pub const ARENA_PAGE: usize = 16 << 10;
/// Pages per arena, page 0 being the header.
pub const ARENA_PAGES: usize = ARENA_LEN / ARENA_PAGE;

/// Bitmap of a fresh arena: every page free but the header.
const ALL_FREE: u64 = !1;

#[repr(C)]
pub struct Arena {
    /// The heap that carves spans here; its blocks come back onto a
    /// span's local list, other heaps' releases take the atomic one.
    pub owner: *mut TlabCache,
    /// Links in the owner's list of arenas with free pages.
    pub next: *mut Arena,
    pub prev: *mut Arena,
    /// Bit `i` set when page `i` is free; bit 0 never is.
    free_pages: u64,
    /// Bit `i` set when free page `i` was handed to the kernel as
    /// reusable and has not been taken back: the kernel counts such
    /// a page out of the footprint until it is told the page is in
    /// use again, whatever is written to it.
    advised: u64,
    /// For each page, the first page of the span covering it.
    page_span: [u8; ARENA_PAGES],
    /// Metadata of the span that starts at each page.
    metas: [SpanMeta; ARENA_PAGES],
}

const _: () = assert!(size_of::<Arena>() <= ARENA_PAGE);
const _: () = assert!(ARENA_PAGES == 64);

/// Lowest `n`-page run of set bits, as a page index.
fn find_run(bits: u64, n: usize) -> Option<usize> {
    let mut m = bits;
    for _ in 1..n {
        m &= m >> 1;
    }
    if m == 0 {
        None
    } else {
        Some(m.trailing_zeros() as usize)
    }
}

#[inline]
fn run_mask(first: usize, n: usize) -> u64 {
    ((1u64 << n) - 1) << first
}

impl Arena {
    /// Map a fresh arena for `owner`. Null when the kernel refuses.
    pub fn create(owner: *mut TlabCache) -> *mut Arena {
        let Ok(raw) = mmap_anon_rw(2 * ARENA_LEN) else {
            return ptr::null_mut();
        };
        let start = raw as usize;
        let base = (start + ARENA_LEN - 1) & !(ARENA_LEN - 1);
        let end = start + 2 * ARENA_LEN;
        if base > start {
            let _ = unsafe { munmap(raw, base - start) };
        }
        if base + ARENA_LEN < end {
            let _ = unsafe { munmap((base + ARENA_LEN) as *mut u8, end - base - ARENA_LEN) };
        }
        // the mapping is zero-filled: links, page table and metadata
        // all start out as nulls, zeros and `SpanState::Free`
        let arena = base as *mut Arena;
        unsafe {
            (*arena).owner = owner;
            (*arena).free_pages = ALL_FREE;
        }
        arena
    }

    /// Unmap an arena; nothing of it may be referenced afterwards.
    ///
    /// # Safety
    /// `arena` came from [`Arena::create`] and is in no list.
    pub unsafe fn destroy(arena: *mut Arena) {
        let _ = unsafe { munmap(arena as *mut u8, ARENA_LEN) };
    }

    /// The arena holding `p`, which must be a block of some arena.
    #[inline]
    pub fn containing(p: *mut u8) -> *mut Arena {
        ((p as usize) & !(ARENA_LEN - 1)) as *mut Arena
    }

    #[inline]
    pub fn base(&self) -> *mut u8 {
        self as *const Arena as *mut u8
    }

    /// Carve a span of `class`. Null when no run of pages fits.
    pub fn alloc_span(&mut self, class: usize) -> *mut SpanMeta {
        let pages = span_pages(class);
        if pages == 0 {
            return ptr::null_mut();
        }
        let Some(first) = find_run(self.free_pages, pages) else {
            return ptr::null_mut();
        };
        let mask = run_mask(first, pages);
        self.free_pages &= !mask;
        if self.advised & mask != 0 {
            let run = unsafe { self.base().add(first * ARENA_PAGE) };
            let _ = unsafe { madvise(run, pages * ARENA_PAGE, MADV_FREE_REUSE) };
            self.advised &= !mask;
        }
        // SAFETY: first + pages <= ARENA_PAGES, the bitmap's width
        unsafe {
            for i in first..first + pages {
                *self.page_span.get_unchecked_mut(i) = first as u8;
            }
            let meta = self.metas.get_unchecked_mut(first);
            meta.init(class, pages, slots_per_span(class));
            meta
        }
    }

    /// Return a span's pages to the bitmap.
    ///
    /// # Safety
    /// `meta` is a span of this arena with no block outstanding.
    pub unsafe fn free_span(&mut self, meta: *mut SpanMeta) {
        let first = self.index_of(meta);
        let pages = unsafe { (*meta).pages() };
        self.free_pages |= run_mask(first, pages);
    }

    /// Hand a span's pages to the kernel as reusable: out of the
    /// footprint at once, reclaimable under pressure, still mapped.
    /// `alloc_span` tells the kernel when it writes them again.
    ///
    /// # Safety
    /// `meta` is a span of this arena with no block outstanding.
    pub unsafe fn advise_span_reusable(&mut self, meta: *mut SpanMeta) {
        let first = self.index_of(meta);
        let pages = unsafe { (*meta).pages() };
        let run = unsafe { self.base().add(first * ARENA_PAGE) };
        if unsafe { madvise(run, pages * ARENA_PAGE, MADV_FREE_REUSABLE) }.is_ok() {
            self.advised |= run_mask(first, pages);
        }
    }

    /// Pages given to the kernel as reusable and not yet taken back.
    #[inline]
    pub fn advised_page_count(&self) -> usize {
        self.advised.count_ones() as usize
    }

    /// The span holding `p`.
    ///
    /// # Safety
    /// `p` points into this arena past the header page.
    #[inline]
    pub unsafe fn meta_of(&mut self, p: *mut u8) -> *mut SpanMeta {
        let page = (p as usize - self.base() as usize) / ARENA_PAGE;
        // SAFETY: p is inside the arena, so page < ARENA_PAGES, and
        // every page a block can sit on was written by alloc_span
        unsafe {
            let first = *self.page_span.get_unchecked(page) as usize;
            self.metas.get_unchecked_mut(first)
        }
    }

    /// First byte of the span `meta` describes.
    #[inline]
    pub fn span_base(&self, meta: *const SpanMeta) -> *mut u8 {
        let first = self.index_of(meta);
        unsafe { self.base().add(first * ARENA_PAGE) }
    }

    #[inline]
    fn index_of(&self, meta: *const SpanMeta) -> usize {
        (meta as usize - self.metas.as_ptr() as usize) / size_of::<SpanMeta>()
    }

    /// Every page but the header is free.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.free_pages == ALL_FREE
    }

    #[inline]
    pub fn has_free_pages(&self) -> bool {
        self.free_pages != 0
    }

    #[inline]
    pub fn free_page_count(&self) -> usize {
        self.free_pages.count_ones() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::size_class::N_CLASSES;

    #[test]
    fn create_gives_an_aligned_zeroed_arena_with_every_page_free() {
        let a = Arena::create(ptr::null_mut());
        assert!(!a.is_null());
        assert_eq!(a as usize % ARENA_LEN, 0);
        let arena = unsafe { &mut *a };
        assert!(arena.is_empty());
        assert_eq!(arena.free_page_count(), ARENA_PAGES - 1);
        assert!(arena.next.is_null() && arena.prev.is_null());
        // the last byte of the arena is mapped and writable
        unsafe { *arena.base().add(ARENA_LEN - 1) = 1 };
        unsafe { Arena::destroy(a) };
    }

    #[test]
    fn find_run_takes_the_lowest_fit() {
        assert_eq!(find_run(ALL_FREE, 1), Some(1));
        assert_eq!(find_run(ALL_FREE, 8), Some(1));
        // pages 1..=2 free, 3 taken, 4..=7 free
        let bits = 0b1111_0110u64;
        assert_eq!(find_run(bits, 1), Some(1));
        assert_eq!(find_run(bits, 2), Some(1));
        assert_eq!(find_run(bits, 3), Some(4));
        assert_eq!(find_run(bits, 4), Some(4));
        assert_eq!(find_run(bits, 5), None);
        assert_eq!(find_run(0, 1), None);
        // a run may end on the last page
        assert_eq!(find_run(0xffu64 << 56, 8), Some(56));
    }

    #[test]
    fn alloc_span_carves_whole_runs_and_free_span_returns_them() {
        let a = Arena::create(ptr::null_mut());
        let arena = unsafe { &mut *a };
        let one = arena.alloc_span(0);
        let eight = arena.alloc_span(8);
        assert!(!one.is_null() && !eight.is_null());
        assert_eq!(arena.free_page_count(), ARENA_PAGES - 1 - 1 - 8);
        assert_eq!(arena.span_base(one), unsafe {
            arena.base().add(ARENA_PAGE)
        });
        assert_eq!(arena.span_base(eight), unsafe {
            arena.base().add(2 * ARENA_PAGE)
        });
        // every page of the 8-page span maps back to its metadata
        for page in 2..10 {
            let p = unsafe { arena.base().add(page * ARENA_PAGE + 100) };
            assert_eq!(unsafe { arena.meta_of(p) }, eight, "page {page}");
        }
        assert_eq!(unsafe { arena.meta_of(arena.base().add(ARENA_PAGE)) }, one);
        unsafe { arena.free_span(eight) };
        assert_eq!(arena.free_page_count(), ARENA_PAGES - 2);
        unsafe { arena.free_span(one) };
        assert!(arena.is_empty());
        unsafe { Arena::destroy(a) };
    }

    /// Pages advised reusable are remembered until a span takes them
    /// again, and the block there is writable afterwards.
    #[test]
    fn advised_pages_are_taken_back_when_carved_again() {
        let a = Arena::create(ptr::null_mut());
        let arena = unsafe { &mut *a };
        let one = arena.alloc_span(0);
        let eight = arena.alloc_span(8);
        unsafe { *arena.span_base(eight) = 0x5a };
        unsafe { arena.advise_span_reusable(eight) };
        unsafe { arena.free_span(eight) };
        assert_eq!(arena.advised_page_count(), 8);
        // the same run comes back, taken back from the kernel
        let again = arena.alloc_span(8);
        assert_eq!(again, eight);
        assert_eq!(arena.advised_page_count(), 0);
        unsafe {
            *arena.span_base(again).add(8 * ARENA_PAGE - 1) = 0x33;
            assert_eq!(*arena.span_base(again).add(8 * ARENA_PAGE - 1), 0x33);
        }
        // a span freed without advice adds nothing; a run partly over
        // advised pages (a 4-page span at pages 1..=4, advised 2..=9)
        // clears its part
        unsafe { arena.advise_span_reusable(again) };
        unsafe { arena.free_span(again) };
        unsafe { arena.free_span(one) };
        assert_eq!(arena.advised_page_count(), 8);
        assert_eq!(span_pages(7), 4);
        let four = arena.alloc_span(7);
        assert_eq!(arena.span_base(four), unsafe {
            arena.base().add(ARENA_PAGE)
        });
        assert_eq!(arena.advised_page_count(), 5);
        unsafe { Arena::destroy(a) };
    }

    #[test]
    fn alloc_span_answers_null_when_no_run_fits() {
        let a = Arena::create(ptr::null_mut());
        let arena = unsafe { &mut *a };
        let mut n = 0;
        while !arena.alloc_span(0).is_null() {
            n += 1;
        }
        assert_eq!(n, ARENA_PAGES - 1);
        assert!(!arena.has_free_pages());
        unsafe { Arena::destroy(a) };
    }

    #[test]
    fn containing_recovers_the_arena_from_any_block() {
        let a = Arena::create(ptr::null_mut());
        let arena = unsafe { &mut *a };
        for class in 0..N_CLASSES {
            let meta = arena.alloc_span(class);
            let base = arena.span_base(meta);
            assert_eq!(Arena::containing(base), a);
            assert_eq!(Arena::containing(unsafe { base.add(17) }), a);
            assert_eq!(unsafe { (*meta).class() }, class);
        }
        unsafe { Arena::destroy(a) };
    }
}
