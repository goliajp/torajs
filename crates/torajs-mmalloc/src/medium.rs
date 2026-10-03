//! The medium path: blocks past the small classes, up to
//! `MAX_MEDIUM`, served one slot at a time from spans of the same
//! arenas the small classes use.
//!
//! No row sits in front of these classes. A row exists so the
//! toolchain can inline an allocation into eight instructions; a
//! medium block costs more to fill than to find, so the span's own
//! free list is the cache: an allocation pops the current span, a
//! release pushes the block's span by the owner's path or the atomic
//! one, and the span moves between the owner's lists as the small
//! ones do.
//!
//! A medium span that empties out gives its pages back to its arena
//! and tells the kernel they are reusable, so they leave the
//! process's footprint at once; the arena itself stays mapped. The
//! exception is the class's current span, kept with its pages
//! resident while the pages the kept spans have written together fit
//! `KEPT_BUDGET_PAGES`: a program that builds and drops one buffer in
//! a loop, through the one or two classes a doubling growth passes,
//! keeps hitting the same pages with no call into the kernel, while
//! one that grows a buffer through every class holds back only the
//! newest spans that fit. mimalloc keeps an empty page the same way
//! while it is alone in its queue, below `MI_MAX_RETIRE_SIZE`; here
//! the oldest kept span goes when the budget is exceeded, and a span
//! that has written more than the budget is not kept at all. The
//! budget is less than what a bench cell may grow by in peak
//! footprint before its gate fails.

use core::ptr;

use crate::arena::Arena;
use crate::size_class::class_size;
use crate::span::{SpanMeta, SpanState};
use crate::tlab::TlabCache;

/// Written pages the kept spans may hold in all: 48 KiB, one block
/// each of two adjacent classes up to 20 KiB, or one up to 48 KiB.
const KEPT_BUDGET_PAGES: usize = 3;

impl TlabCache {
    /// One block of medium `class`. Null when the kernel refuses more
    /// memory.
    pub(crate) fn alloc_medium(&mut self, class: usize) -> *mut u8 {
        let size = class_size(class);
        loop {
            let mut span = self.cur(class);
            if span.is_null() {
                span = self.open_span(class);
                if span.is_null() {
                    return ptr::null_mut();
                }
            }
            let meta = unsafe { &mut *span };
            if meta.state() == SpanState::Kept {
                self.kept_unlink(span);
                meta.set_state(SpanState::Cur);
            }
            let base = unsafe { (*Arena::containing(span as *mut u8)).span_base(span) };
            if !meta.has_local() {
                meta.collect_remote();
            }
            let p = unsafe { meta.take_one(base, size) };
            if p.is_null() {
                meta.set_state(SpanState::Full);
                self.set_cur(class, ptr::null_mut());
                continue;
            }
            return p;
        }
    }

    /// A block of medium `class` goes back to its span. The span's
    /// own class drives the bookkeeping; `class` is what the caller
    /// freed it as, checked under the guard.
    ///
    /// # Safety
    /// `p` is a live block of `class` from this allocator.
    pub(crate) unsafe fn free_medium(&mut self, class: usize, p: *mut u8) {
        let arena = Arena::containing(p);
        let meta = unsafe { (*arena).meta_of(p) };
        #[cfg(torajs_alloc_guard)]
        assert!(
            unsafe { (*meta).class() } == class,
            "block released at another size class"
        );
        #[cfg(not(torajs_alloc_guard))]
        let _ = class;
        if !ptr::eq(unsafe { (*arena).owner }, self) {
            unsafe { (*meta).release_remote(p) };
            return;
        }
        let class = unsafe { (*meta).class() };
        let state = unsafe { (*meta).state() };
        unsafe {
            (*meta).push_chain(p, p, 1);
            (*meta).take_back(1);
        }
        if unsafe { (*meta).used() } == 0 {
            let written = unsafe { (*meta).touched_pages(class_size(class)) };
            if state == SpanState::Cur && written <= KEPT_BUDGET_PAGES {
                self.keep(meta, written);
            } else {
                unsafe { self.retire_medium(class, meta, arena) };
            }
        } else if state == SpanState::Full {
            self.partial_push(class, meta);
        }
    }

    /// The pages of an empty medium span go back to the arena and
    /// out of the footprint.
    unsafe fn retire_medium(&mut self, class: usize, meta: *mut SpanMeta, arena: *mut Arena) {
        unsafe { (*arena).advise_span_reusable(meta) };
        unsafe { self.retire(class, meta, arena) };
    }

    /// Hold an empty current span that has `written` pages back,
    /// newest first, retiring the oldest kept spans until it fits.
    fn keep(&mut self, meta: *mut SpanMeta, written: usize) {
        while self.kept_pages + written > KEPT_BUDGET_PAGES {
            let old = self.kept_tail;
            let class = unsafe { (*old).class() };
            unsafe { self.retire_medium(class, old, Arena::containing(old as *mut u8)) };
        }
        unsafe {
            (*meta).set_state(SpanState::Kept);
            (*meta).prev = ptr::null_mut();
            (*meta).next = self.kept;
            if self.kept.is_null() {
                self.kept_tail = meta;
            } else {
                (*self.kept).prev = meta;
            }
        }
        self.kept = meta;
        self.kept_pages += written;
    }

    /// Take a span out of the kept list; nothing is carved while a
    /// span is kept, so its written pages are what `keep` counted.
    pub(crate) fn kept_unlink(&mut self, meta: *mut SpanMeta) {
        unsafe {
            let (prev, next) = ((*meta).prev, (*meta).next);
            if prev.is_null() {
                self.kept = next;
            } else {
                (*prev).next = next;
            }
            if next.is_null() {
                self.kept_tail = prev;
            } else {
                (*next).prev = prev;
            }
        }
        let size = class_size(unsafe { (*meta).class() });
        self.kept_pages -= unsafe { (*meta).touched_pages(size) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ARENA_PAGES;
    use crate::region_registry::with_registry;
    use crate::size_class::{N_ALL, N_CLASSES, slots_per_span, span_pages};
    use crate::span::SpanMeta;

    // a heap must not move once arenas record it as their owner
    fn heap() -> Box<TlabCache> {
        Box::new(TlabCache::new())
    }

    fn alloc(h: &mut TlabCache, class: usize) -> *mut u8 {
        let p = h.alloc_medium(class);
        assert!(!p.is_null());
        p
    }

    fn meta_of(p: *mut u8) -> *mut SpanMeta {
        unsafe { (*Arena::containing(p)).meta_of(p) }
    }

    // 10 KiB: 6 slots over 4 pages; one block writes 1 page
    const C10K: usize = N_CLASSES + 4;
    // 24 KiB: 2 slots over 3 pages; one block writes 2 pages
    const C24K: usize = N_CLASSES + 9;
    // 48 KiB: one slot over 3 pages, the widest block kept when empty
    const C48K: usize = N_CLASSES + 13;
    // 64 KiB: one slot over 4 pages, one more than the budget
    const C64K: usize = N_CLASSES + 15;
    // 80 KiB: one slot over 5 pages, retired as soon as it empties
    const C80K: usize = N_CLASSES + 16;

    #[test]
    fn classes_this_test_assumes() {
        assert_eq!(class_size(C10K), 10 << 10);
        assert_eq!((span_pages(C10K), slots_per_span(C10K)), (4, 6));
        assert_eq!(class_size(C24K), 24 << 10);
        assert_eq!((span_pages(C24K), slots_per_span(C24K)), (3, 2));
        assert_eq!(class_size(C48K), 48 << 10);
        assert_eq!((span_pages(C48K), slots_per_span(C48K)), (3, 1));
        assert_eq!(span_pages(C48K), KEPT_BUDGET_PAGES);
        assert_eq!(class_size(C64K), 64 << 10);
        assert_eq!((span_pages(C64K), slots_per_span(C64K)), (4, 1));
        assert_eq!(class_size(C80K), 80 << 10);
        assert_eq!((span_pages(C80K), slots_per_span(C80K)), (5, 1));
    }

    #[test]
    fn alloc_walks_the_span_then_fills_a_second_one() {
        let mut h = heap();
        let n = slots_per_span(C10K);
        let first: Vec<_> = (0..n).map(|_| alloc(&mut h, C10K)).collect();
        let a = meta_of(first[0]);
        for (i, &p) in first.iter().enumerate() {
            assert!(ptr::eq(meta_of(p), a));
            assert_eq!(p as usize - first[0] as usize, i * class_size(C10K));
            assert_eq!(p as usize % 16, 0);
        }
        assert_eq!(unsafe { (*a).used() }, n);
        assert_eq!(unsafe { (*a).state() }, SpanState::Cur);
        let extra = alloc(&mut h, C10K);
        let b = meta_of(extra);
        assert!(!ptr::eq(a, b));
        assert_eq!(unsafe { (*a).state() }, SpanState::Full);
        assert_eq!(h.cur(C10K), b);
        assert_eq!(h.listed_arenas(), 1);
    }

    /// Alternating alloc / free hands back the same block from the
    /// same span: no retire, no carve, no new mapping.
    #[test]
    fn an_empty_current_span_is_kept_and_reused() {
        let mut h = heap();
        let p = alloc(&mut h, C10K);
        let m = meta_of(p);
        let arena = Arena::containing(p);
        let pages = unsafe { (*arena).free_page_count() };
        let regions = with_registry(|r| r.len());
        for _ in 0..1000 {
            unsafe { h.free_medium(C10K, p) };
            assert_eq!(unsafe { (*m).used() }, 0);
            assert_eq!(unsafe { (*m).state() }, SpanState::Kept);
            assert_eq!(h.cur(C10K), m);
            assert_eq!((h.kept, h.kept_tail, h.kept_pages), (m, m, 1));
            assert_eq!(alloc(&mut h, C10K), p);
            assert_eq!(unsafe { (*m).state() }, SpanState::Cur);
            assert!(h.kept.is_null() && h.kept_tail.is_null());
            assert_eq!(h.kept_pages, 0);
        }
        assert_eq!(unsafe { (*arena).free_page_count() }, pages);
        assert_eq!(with_registry(|r| r.len()), regions);
        // the same for the widest kept class, which fills the budget
        let q = alloc(&mut h, C48K);
        let qm = meta_of(q);
        for _ in 0..100 {
            unsafe { h.free_medium(C48K, q) };
            assert_eq!(unsafe { (*qm).state() }, SpanState::Kept);
            assert_eq!(h.kept_pages, KEPT_BUDGET_PAGES);
            assert_eq!(alloc(&mut h, C48K), q);
        }
    }

    /// Kept spans are a budgeted, newest-first list of written
    /// pages: two small blocks fit together, a block that does not
    /// fit beside them retires the oldest first, one that has
    /// written more than the budget is never kept, and taking a
    /// block from a kept span moves it out of the list.
    #[test]
    fn kept_spans_stay_within_the_budget_oldest_out() {
        let mut h = heap();
        let a = alloc(&mut h, C10K);
        let b = alloc(&mut h, C24K);
        let c = alloc(&mut h, C48K);
        let d = alloc(&mut h, C64K);
        let (ma, mb, mc, md) = (meta_of(a), meta_of(b), meta_of(c), meta_of(d));
        unsafe { h.free_medium(C10K, a) };
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (ma, ma, 1));
        unsafe { h.free_medium(C24K, b) };
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (mb, ma, 3));
        assert_eq!(unsafe { (*mb).next }, ma);
        assert_eq!(unsafe { (*ma).prev }, mb);
        // c needs the whole budget: a and b go, oldest first, and
        // their classes have no current span
        unsafe { h.free_medium(C48K, c) };
        assert_eq!(unsafe { (*ma).state() }, SpanState::Free);
        assert_eq!(unsafe { (*mb).state() }, SpanState::Free);
        assert!(h.cur(C10K).is_null() && h.cur(C24K).is_null());
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (mc, mc, 3));
        // d has written more than the budget: retired at once
        unsafe { h.free_medium(C64K, d) };
        assert_eq!(unsafe { (*md).state() }, SpanState::Free);
        assert!(h.cur(C64K).is_null());
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (mc, mc, 3));
        // taking from c makes it current again and out of the list
        assert_eq!(alloc(&mut h, C48K), c);
        assert_eq!(unsafe { (*mc).state() }, SpanState::Cur);
        assert!(h.kept.is_null() && h.kept_tail.is_null());
        assert_eq!(h.kept_pages, 0);
        // a fresh 10 KiB block comes from a new span and is kept;
        // c kept after it evicts it
        let a2 = alloc(&mut h, C10K);
        unsafe { h.free_medium(C10K, a2) };
        assert_eq!(
            (h.kept, h.kept_tail, h.kept_pages),
            (meta_of(a2), meta_of(a2), 1)
        );
        unsafe { h.free_medium(C48K, c) };
        assert_eq!(unsafe { (*meta_of(a2)).state() }, SpanState::Free);
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (mc, mc, 3));
        // a span that has carved past the budget is not kept either
        let many: Vec<_> = (0..slots_per_span(C10K))
            .map(|_| alloc(&mut h, C10K))
            .collect();
        let mm = meta_of(many[0]);
        assert_eq!(unsafe { (*mm).touched_pages(class_size(C10K)) }, 4);
        for &p in &many {
            unsafe { h.free_medium(C10K, p) };
        }
        assert_eq!(unsafe { (*mm).state() }, SpanState::Free);
        assert_eq!((h.kept, h.kept_tail, h.kept_pages), (mc, mc, 3));
    }

    /// Past the kept size an empty span gives its pages back at once,
    /// and the next allocation carves them again from the same
    /// arena, which stays mapped.
    #[test]
    fn a_large_empty_span_retires_and_its_pages_are_recarved() {
        let mut h = heap();
        let p = alloc(&mut h, C80K);
        let arena = Arena::containing(p);
        let m = meta_of(p);
        let free_before = unsafe { (*arena).free_page_count() };
        unsafe { h.free_medium(C80K, p) };
        assert_eq!(unsafe { (*m).state() }, SpanState::Free);
        assert!(h.cur(C80K).is_null());
        assert_eq!(
            unsafe { (*arena).free_page_count() },
            free_before + span_pages(C80K)
        );
        assert!(unsafe { (*arena).is_empty() });
        assert_eq!(unsafe { (*arena).advised_page_count() }, span_pages(C80K));
        assert_eq!(h.spare, arena, "the only arena is kept as the spare");
        let q = alloc(&mut h, C80K);
        assert_eq!(q, p, "the same pages come back");
        assert_eq!(unsafe { (*arena).advised_page_count() }, 0);
        assert!(h.spare.is_null());
    }

    /// A span in the partial list that empties out retires too.
    #[test]
    fn a_partial_span_that_empties_retires() {
        let mut h = heap();
        let n = slots_per_span(C10K);
        let first: Vec<_> = (0..n).map(|_| alloc(&mut h, C10K)).collect();
        let a = meta_of(first[0]);
        let extra = alloc(&mut h, C10K);
        assert_eq!(unsafe { (*a).state() }, SpanState::Full);
        unsafe { h.free_medium(C10K, first[0]) };
        assert_eq!(unsafe { (*a).state() }, SpanState::Partial);
        assert_eq!(h.partial[C10K], a);
        // the partial span is preferred over the current span's tail
        // once the current one runs dry: fill b, then the next block
        // is a's
        let b = meta_of(extra);
        for _ in 1..n {
            assert!(ptr::eq(meta_of(alloc(&mut h, C10K)), b));
        }
        let back = alloc(&mut h, C10K);
        assert_eq!(back, first[0]);
        assert_eq!(unsafe { (*b).state() }, SpanState::Full);
        assert_eq!(h.cur(C10K), a);
        assert!(h.partial[C10K].is_null());
        // b empties out from the partial list: pages back
        unsafe { h.free_medium(C10K, extra) };
        assert_eq!(unsafe { (*b).state() }, SpanState::Partial);
        let arena = Arena::containing(extra);
        let pages = unsafe { (*arena).free_page_count() };
        let b_blocks: Vec<_> = (1..n)
            .map(|i| unsafe { extra.add(i * class_size(C10K)) })
            .collect();
        for &p in &b_blocks {
            unsafe { h.free_medium(C10K, p) };
        }
        assert_eq!(unsafe { (*b).state() }, SpanState::Free);
        assert!(h.partial[C10K].is_null());
        assert_eq!(
            unsafe { (*arena).free_page_count() },
            pages + span_pages(C10K)
        );
    }

    #[test]
    fn a_block_of_another_heap_goes_on_the_atomic_list() {
        let mut owner = heap();
        let mut other = heap();
        let p = alloc(&mut owner, C10K);
        let m = meta_of(p);
        unsafe { other.free_medium(C10K, p) };
        assert_eq!(unsafe { (*m).used() }, 1, "used is the owner's");
        assert_eq!(unsafe { (*m).n_free() }, 0);
        // the owner's next allocation collects it
        let n = slots_per_span(C10K);
        for _ in 1..n {
            alloc(&mut owner, C10K);
        }
        assert_eq!(unsafe { (*m).used() }, n);
        let q = alloc(&mut owner, C10K);
        assert_eq!(q, p, "the remote release is collected before a new span");
        assert_eq!(unsafe { (*m).used() }, n);
        assert!(other.cur(C10K).is_null());
    }

    #[test]
    fn every_medium_class_hands_out_aligned_writable_blocks() {
        let mut h = heap();
        for class in N_CLASSES..N_ALL {
            let size = class_size(class);
            let mut blocks = Vec::new();
            for _ in 0..3 {
                let p = alloc(&mut h, class);
                assert_eq!(p as usize % 16, 0);
                unsafe {
                    ptr::write_bytes(p, 0x5a, size);
                    assert_eq!(*p.add(size - 1), 0x5a);
                }
                assert_eq!(unsafe { (*meta_of(p)).class() }, class);
                blocks.push(p);
            }
            for p in blocks {
                unsafe { h.free_medium(class, p) };
            }
        }
    }

    /// Many blocks of one class fill arenas; freeing them all retires
    /// every span but the kept current one, and the arenas go back to
    /// the kernel past the spare.
    #[test]
    fn freeing_everything_returns_arenas_past_the_spare() {
        let mut h = heap();
        let per_arena = (ARENA_PAGES - 1) / span_pages(C48K);
        let regions = with_registry(|r| r.len());
        let ptrs: Vec<_> = (0..3 * per_arena).map(|_| alloc(&mut h, C48K)).collect();
        assert_eq!(with_registry(|r| r.len()), regions + 3);
        for &p in ptrs.iter().rev() {
            unsafe { h.free_medium(C48K, p) };
        }
        // one span kept (its arena listed), one spare arena, the
        // third unmapped
        assert_eq!(with_registry(|r| r.len()), regions + 2);
        assert!(!h.spare.is_null());
        let kept = h.cur(C48K);
        assert!(!kept.is_null());
        assert_eq!(unsafe { (*kept).used() }, 0);
        assert_eq!(unsafe { (*kept).state() }, SpanState::Kept);
        assert_eq!(h.kept, kept);
        assert_eq!(
            unsafe { (*Arena::containing(kept as *mut u8)).free_page_count() },
            ARENA_PAGES - 1 - span_pages(C48K)
        );
    }
}
