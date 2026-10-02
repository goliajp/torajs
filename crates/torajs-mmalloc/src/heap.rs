//! The heap's slow path: what happens when a row runs dry or over.
//!
//! Refill takes a whole chain from the class's current span: its
//! local free list in one splice, else anything other threads
//! released to it, else a carve from its never-used tail; a span with
//! nothing left is `Full` and the class moves to its first partial
//! span or a fresh one. Spill moves the newest half of a full row back
//! to the blocks' spans: a block of this heap's own arena goes on its
//! span's local list, and a span that empties out gives its pages back
//! to its arena. A block from another heap's arena goes on its span's
//! atomic list instead.
//!
//! Arenas with free pages hang off the heap in a list; an arena that
//! empties out is kept as the one spare, and the next empty arena is
//! unmapped.

use core::ptr;

use crate::arena::{ARENA_LEN, ARENA_PAGE, Arena};
use crate::region_registry::{RegionKind, with_registry};
use crate::size_class::class_size;
use crate::span::{SpanMeta, SpanState};
use crate::tlab::{FREE_LINK_OFF, SPILL_AT, TlabCache};

/// Blocks a spill moves out of a full row.
const SPILL_COUNT: usize = SPILL_AT / 2;

#[inline]
unsafe fn link(p: *mut u8) -> *mut *mut u8 {
    unsafe { p.add(FREE_LINK_OFF) as *mut *mut u8 }
}

impl TlabCache {
    /// Refill the empty row of `class` and hand one block out. Null
    /// when the kernel refuses more memory.
    pub(crate) fn refill(&mut self, class: usize) -> *mut u8 {
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
            let base = unsafe { (*Arena::containing(span as *mut u8)).span_base(span) };
            if !meta.has_local() {
                meta.collect_remote();
            }
            let (head, n) = unsafe { meta.take_chain(base, size) };
            if n == 0 {
                meta.set_state(SpanState::Full);
                self.set_cur(class, ptr::null_mut());
                continue;
            }
            self.splice(class, head, n);
            return match self.pop(class) {
                Some(p) => p,
                None => ptr::null_mut(),
            };
        }
    }

    /// The row of `class` is at `SPILL_AT`: put `ptr` and the row's
    /// newest half back on their spans.
    ///
    /// # Safety
    /// `ptr` is a live block of `class` from this allocator.
    pub(crate) unsafe fn spill(&mut self, class: usize, ptr: *mut u8) {
        #[cfg(torajs_census)]
        crate::core::census::SPILLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // ptr joins the row's chain in front; the chain's first
        // SPILL_COUNT + 1 blocks go back, the rest stays the row
        let (head, len) = self.row(class);
        // a full row's push refuses before it looks at the row, so
        // a block already on it would close the chain into a ring
        #[cfg(torajs_alloc_guard)]
        {
            let mut q = head;
            while !q.is_null() {
                assert!(q != ptr, "block freed twice (row)");
                q = unsafe { *link(q) };
            }
        }
        unsafe { *link(ptr) = head };
        let n = SPILL_COUNT.min(len);
        let rest = unsafe { self.release_chain(class, ptr, n + 1) };
        self.splice(class, rest, len - n);
    }

    /// One block back to its span, by the owner's or the foreign path.
    ///
    /// # Safety
    /// As [`TlabCache::spill`].
    #[cfg(test)]
    pub(crate) unsafe fn release_block(&mut self, class: usize, p: *mut u8) {
        unsafe { self.release_chain(class, p, 1) };
    }

    /// The first `n` blocks of the chain at `head` go back to their
    /// spans; returns the block after them. A run of neighbours on
    /// the same span keeps its links and moves onto that span's free
    /// list in one splice, settling the span's count once; a block of
    /// another heap's arena takes the atomic path by itself.
    ///
    /// # Safety
    /// As [`TlabCache::spill`], for every block; the chain holds at
    /// least `n` blocks.
    unsafe fn release_chain(&mut self, class: usize, head: *mut u8, n: usize) -> *mut u8 {
        // the open run: its span, the span's byte range, and the
        // chain segment collected so far
        let mut cur: *mut SpanMeta = ptr::null_mut();
        let mut lo = 0usize;
        let mut len = 0usize;
        let mut was_full = false;
        let mut seg_head = ptr::null_mut();
        let mut seg_last = ptr::null_mut();
        let mut run = 0;
        let mut p = head;
        for _ in 0..n {
            let next = unsafe { *link(p) };
            if (p as usize).wrapping_sub(lo) >= len {
                if run > 0 {
                    unsafe { self.settle_chain(class, cur, seg_head, seg_last, run, was_full) };
                    run = 0;
                }
                let arena = Arena::containing(p);
                let meta = unsafe { (*arena).meta_of(p) };
                #[cfg(torajs_alloc_guard)]
                assert!(
                    unsafe { (*meta).class() } == class,
                    "block released at another size class"
                );
                if !ptr::eq(unsafe { (*arena).owner }, self) {
                    unsafe { (*meta).release_remote(p) };
                    len = 0;
                    p = next;
                    continue;
                }
                cur = meta;
                lo = unsafe { (*arena).span_base(meta) } as usize;
                len = unsafe { (*meta).pages() } * ARENA_PAGE;
                was_full = unsafe { (*meta).state() } == SpanState::Full;
                seg_head = p;
            }
            seg_last = p;
            run += 1;
            p = next;
        }
        if run > 0 {
            unsafe { self.settle_chain(class, cur, seg_head, seg_last, run, was_full) };
        }
        p
    }

    /// A chain segment of `run` blocks goes onto `meta`'s free list,
    /// then the span's count is settled.
    unsafe fn settle_chain(
        &mut self,
        class: usize,
        meta: *mut SpanMeta,
        head: *mut u8,
        last: *mut u8,
        run: usize,
        was_full: bool,
    ) {
        unsafe { (*meta).push_chain(head, last, run) };
        unsafe { self.settle(class, meta, run, was_full) };
    }

    /// `run` blocks went back on `meta`'s free list: count them, and
    /// move the span between the owner's lists if that changed it.
    unsafe fn settle(&mut self, class: usize, meta: *mut SpanMeta, run: usize, was_full: bool) {
        unsafe { (*meta).take_back(run) };
        // a whole span can come back in one run, straight from full
        if unsafe { (*meta).used() } == 0 {
            unsafe { self.retire(class, meta, Arena::containing(meta as *mut u8)) };
        } else if was_full {
            self.partial_push(class, meta);
        }
    }

    /// A span with nothing out: pages back to the arena, and the
    /// arena back to the kernel once it is empty and a spare exists.
    unsafe fn retire(&mut self, class: usize, meta: *mut SpanMeta, arena: *mut Arena) {
        #[cfg(torajs_census)]
        crate::core::census::SPAN_RETIRE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        match unsafe { (*meta).state() } {
            SpanState::Partial => self.partial_unlink(class, meta),
            SpanState::Cur => self.set_cur(class, ptr::null_mut()),
            SpanState::Full | SpanState::Free => {}
        }
        unsafe { (*meta).set_state(SpanState::Free) };
        let was_listed = unsafe { (*arena).has_free_pages() };
        unsafe { (*arena).free_span(meta) };
        if unsafe { (*arena).is_empty() } {
            if was_listed {
                self.arena_unlink(arena);
            }
            if self.spare.is_null() {
                self.spare = arena;
            } else {
                with_registry(|r| r.remove(arena as usize));
                unsafe { Arena::destroy(arena) };
            }
        } else if !was_listed {
            self.arena_link(arena);
        }
    }

    /// Make a span current for `class`: the first partial one, else a
    /// fresh one. Null when no memory can be had.
    fn open_span(&mut self, class: usize) -> *mut SpanMeta {
        let mut span = self.partial_pop(class);
        if span.is_null() {
            span = self.new_span(class);
        }
        self.set_cur(class, span);
        span
    }

    /// Carve a span of `class` from an arena with room, the spare, or
    /// a new mapping.
    fn new_span(&mut self, class: usize) -> *mut SpanMeta {
        let mut a = self.arenas;
        while !a.is_null() {
            let span = self.carve(a, class);
            if !span.is_null() {
                return span;
            }
            a = unsafe { (*a).next };
        }
        let a = if self.spare.is_null() {
            let a = Arena::create(self);
            if a.is_null() {
                return ptr::null_mut();
            }
            if !with_registry(|r| r.insert(a as usize, ARENA_LEN, RegionKind::Arena)) {
                unsafe { Arena::destroy(a) };
                return ptr::null_mut();
            }
            a
        } else {
            core::mem::replace(&mut self.spare, ptr::null_mut())
        };
        self.arena_link(a);
        self.carve(a, class)
    }

    fn carve(&mut self, arena: *mut Arena, class: usize) -> *mut SpanMeta {
        let span = unsafe { (*arena).alloc_span(class) };
        if !span.is_null() && !unsafe { (*arena).has_free_pages() } {
            self.arena_unlink(arena);
        }
        span
    }

    #[inline]
    fn cur(&self, class: usize) -> *mut SpanMeta {
        match self.cur.get(class) {
            Some(&s) => s,
            None => ptr::null_mut(),
        }
    }

    #[inline]
    fn set_cur(&mut self, class: usize, span: *mut SpanMeta) {
        if let Some(slot) = self.cur.get_mut(class) {
            *slot = span;
        }
    }

    fn partial_push(&mut self, class: usize, span: *mut SpanMeta) {
        let Some(head) = self.partial.get_mut(class) else {
            return;
        };
        unsafe {
            (*span).prev = ptr::null_mut();
            (*span).next = *head;
            if !head.is_null() {
                (**head).prev = span;
            }
            (*span).set_state(SpanState::Partial);
        }
        *head = span;
    }

    fn partial_pop(&mut self, class: usize) -> *mut SpanMeta {
        let span = match self.partial.get(class) {
            Some(&s) => s,
            None => ptr::null_mut(),
        };
        if !span.is_null() {
            self.partial_unlink(class, span);
            unsafe { (*span).set_state(SpanState::Cur) };
        }
        span
    }

    fn partial_unlink(&mut self, class: usize, span: *mut SpanMeta) {
        let Some(head) = self.partial.get_mut(class) else {
            return;
        };
        unsafe {
            let (prev, next) = ((*span).prev, (*span).next);
            if prev.is_null() {
                *head = next;
            } else {
                (*prev).next = next;
            }
            if !next.is_null() {
                (*next).prev = prev;
            }
        }
    }

    fn arena_link(&mut self, arena: *mut Arena) {
        unsafe {
            (*arena).prev = ptr::null_mut();
            (*arena).next = self.arenas;
            if !self.arenas.is_null() {
                (*self.arenas).prev = arena;
            }
        }
        self.arenas = arena;
    }

    fn arena_unlink(&mut self, arena: *mut Arena) {
        unsafe {
            let (prev, next) = ((*arena).prev, (*arena).next);
            if prev.is_null() {
                self.arenas = next;
            } else {
                (*prev).next = next;
            }
            if !next.is_null() {
                (*next).prev = prev;
            }
        }
    }

    /// Arenas with free pages, for diagnostics.
    pub fn listed_arenas(&self) -> usize {
        let mut n = 0;
        let mut a = self.arenas;
        while !a.is_null() {
            n += 1;
            a = unsafe { (*a).next };
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ARENA_PAGES;
    use crate::size_class::{slots_per_span, span_pages};

    // a heap must not move once arenas record it as their owner
    fn heap() -> Box<TlabCache> {
        Box::new(TlabCache::new())
    }

    fn alloc(h: &mut TlabCache, class: usize) -> *mut u8 {
        match h.pop(class) {
            Some(p) => p,
            None => {
                let p = h.refill(class);
                assert!(!p.is_null());
                p
            }
        }
    }

    fn free(h: &mut TlabCache, class: usize, p: *mut u8) {
        if !unsafe { h.push(class, p) } {
            unsafe { h.spill(class, p) };
        }
    }

    fn meta_of(p: *mut u8) -> *mut SpanMeta {
        unsafe { (*Arena::containing(p)).meta_of(p) }
    }

    // class 2: 64 B blocks, one page, 256 slots, carved in one go
    const C64: usize = 2;
    // class 8: 4 KiB blocks, 8 pages, 32 slots
    const C4K: usize = 8;

    #[test]
    fn refill_hands_one_out_and_splices_the_rest_of_the_carve() {
        let mut h = heap();
        let p = h.refill(C64);
        assert!(!p.is_null());
        assert_eq!(h.total_cached(), 255);
        let m = meta_of(p);
        assert_eq!(unsafe { (*m).used() }, 256);
        assert_eq!(unsafe { (*m).state() }, SpanState::Cur);
        assert_eq!(h.cur(C64), m);
        assert_eq!(h.listed_arenas(), 1);
        assert_eq!(p as usize % 16, 0);
        // the row walks the carve in address order
        let q = h.pop(C64).unwrap();
        assert_eq!(q as usize - p as usize, 64);
    }

    #[test]
    fn a_full_span_closes_and_a_spill_reopens_it_as_partial() {
        let mut h = heap();
        let n = slots_per_span(C64);
        let first: Vec<_> = (0..n).map(|_| alloc(&mut h, C64)).collect();
        let a = meta_of(first[0]);
        assert!(first.iter().all(|&p| ptr::eq(meta_of(p), a)));
        assert!(h.is_empty(), "256 allocs are exactly one carve");
        // the second span's carve, taken whole, leaves the row empty again
        let extra = alloc(&mut h, C64);
        for _ in 1..n {
            alloc(&mut h, C64);
        }
        assert!(h.is_empty());
        let b = meta_of(extra);
        assert!(
            !ptr::eq(a, b),
            "the second span opens once the first is out"
        );
        assert_eq!(unsafe { (*a).state() }, SpanState::Full);
        assert_eq!(h.cur(C64), b);
        // every block of a fits the row; the next free spills
        for &p in &first {
            free(&mut h, C64, p);
        }
        assert_eq!(h.row_len(C64), SPILL_AT);
        assert_eq!(unsafe { (*a).state() }, SpanState::Full);
        free(&mut h, C64, extra);
        assert_eq!(h.row_len(C64), SPILL_AT - SPILL_COUNT);
        assert_eq!(unsafe { (*a).state() }, SpanState::Partial);
        assert_eq!(h.partial[C64], a);
        assert_eq!(unsafe { (*a).used() }, n - SPILL_COUNT);
        assert_eq!(unsafe { (*a).n_free() }, SPILL_COUNT);
        assert_eq!(unsafe { (*b).n_free() }, 1);
        // the row, then b's one free block, then the class goes back
        // to a's free list in one splice, not to a third span
        let pages_before = unsafe { (*Arena::containing(extra)).free_page_count() };
        for _ in 0..(SPILL_AT - SPILL_COUNT) + 1 {
            alloc(&mut h, C64);
        }
        assert!(h.is_empty());
        let again = alloc(&mut h, C64);
        assert!(ptr::eq(meta_of(again), a));
        assert_eq!(h.cur(C64), a);
        assert!(h.partial[C64].is_null());
        assert_eq!(h.row_len(C64), SPILL_COUNT - 1);
        assert_eq!(unsafe { (*a).n_free() }, 0);
        assert_eq!(
            unsafe { (*Arena::containing(extra)).free_page_count() },
            pages_before
        );
    }

    #[test]
    fn freeing_everything_retires_every_span_the_row_lets_go() {
        let mut h = heap();
        let n = slots_per_span(C64);
        let spans = 10;
        let ptrs: Vec<_> = (0..spans * n).map(|_| alloc(&mut h, C64)).collect();
        let arena = Arena::containing(ptrs[0]);
        assert_eq!(
            unsafe { (*arena).free_page_count() },
            ARENA_PAGES - 1 - spans * span_pages(C64)
        );
        for &p in &ptrs {
            free(&mut h, C64, p);
        }
        // the row keeps its oldest half (the first span's blocks) and
        // whatever came after the last spill; every other block went
        // back, and a span with nothing out returned its page
        let a = meta_of(ptrs[0]);
        assert_eq!(unsafe { (*a).used() }, SPILL_COUNT);
        let mut live = 0;
        let mut out = 0;
        for s in 0..spans {
            let m = meta_of(ptrs[s * n]);
            let used = unsafe { (*m).used() };
            out += used;
            if used == 0 {
                assert_eq!(unsafe { (*m).state() }, SpanState::Free, "span {s}");
            } else {
                live += 1;
            }
        }
        assert_eq!(out, h.total_cached());
        assert!(h.total_cached() < SPILL_AT);
        assert!(live <= 3, "live spans {live}");
        assert_eq!(
            unsafe { (*arena).free_page_count() },
            ARENA_PAGES - 1 - live * span_pages(C64)
        );
    }

    #[test]
    fn empty_arenas_become_the_spare_then_return_to_the_kernel() {
        let mut h = heap();
        let per_arena = ((ARENA_PAGES - 1) / span_pages(C4K)) * slots_per_span(C4K);
        let ptrs: Vec<_> = (0..4 * per_arena).map(|_| alloc(&mut h, C4K)).collect();
        let arenas: Vec<_> = (0..4)
            .map(|i| Arena::containing(ptrs[i * per_arena]))
            .collect();
        for i in 1..4 {
            assert_ne!(arenas[i - 1], arenas[i]);
        }
        assert!(with_registry(|r| r.lookup(arenas[1] as usize)).is_some());
        // freed newest first: the row keeps the fourth arena's last
        // blocks (its oldest half) and the first arena's (pushed after
        // the last spill); the third arena empties out first and is
        // kept as the spare, the second empties out next and is
        // unmapped
        for &p in ptrs.iter().rev() {
            free(&mut h, C4K, p);
        }
        let state: Vec<String> = arenas
            .iter()
            .map(|&a| {
                let reg = with_registry(|r| r.lookup(a as usize)).is_some();
                if reg {
                    format!("{a:p} pages={} ", unsafe { (*a).free_page_count() })
                } else {
                    format!("{a:p} unmapped ")
                }
            })
            .collect();
        assert_eq!(
            h.spare,
            arenas[2],
            "row={} listed={} {}",
            h.total_cached(),
            h.listed_arenas(),
            state.concat()
        );
        assert!(
            with_registry(|r| r.lookup(arenas[1] as usize)).is_none(),
            "the second empty arena is unmapped and deregistered"
        );
        assert_eq!(h.listed_arenas(), 2);
        assert!(with_registry(|r| r.lookup(arenas[2] as usize)).is_some());
        // the listed arenas fill up, then the spare is used before any
        // new arena is mapped
        let before = with_registry(|r| r.len());
        let mut n = 0;
        while !h.spare.is_null() {
            alloc(&mut h, C4K);
            n += 1;
            assert!(n <= 4 * per_arena, "spare never taken");
            assert_eq!(
                with_registry(|r| r.len()),
                before,
                "a new arena before the spare"
            );
        }
        assert!(n > 0);
        assert_eq!(h.listed_arenas(), 3);
    }

    #[test]
    fn a_block_of_another_heap_goes_on_the_atomic_list() {
        let mut owner = heap();
        let mut other = heap();
        let p = alloc(&mut owner, C64);
        let m = meta_of(p);
        unsafe { other.release_block(C64, p) };
        assert_eq!(unsafe { (*m).used() }, 256, "used is the owner's");
        assert_eq!(unsafe { (*m).collect_remote() }, 1);
        assert_eq!(unsafe { (*m).used() }, 255);
        assert_eq!(unsafe { (*m).n_free() }, 1);
        let _ = alloc(&mut other, C64);
    }

    /// Walk a row's chain from its head, collecting the blocks.
    fn row_chain(h: &TlabCache, class: usize) -> Vec<*mut u8> {
        let mut v = Vec::new();
        let (mut p, _) = h.row(class);
        while !p.is_null() {
            v.push(p);
            p = unsafe { *link(p) };
        }
        v
    }

    /// A spill whose chain changes span twice and crosses a block of
    /// another heap: every segment lands on its own span with the
    /// counts right, the foreign block goes on its atomic list, and
    /// the row keeps exactly its older half.
    #[test]
    fn spill_splits_the_chain_where_the_span_changes() {
        let mut h = heap();
        let mut other = heap();
        let n = slots_per_span(C64);
        let s_blocks: Vec<_> = (0..n).map(|_| alloc(&mut h, C64)).collect();
        let t_blocks: Vec<_> = (0..n).map(|_| alloc(&mut h, C64)).collect();
        let s = meta_of(s_blocks[0]);
        let t = meta_of(t_blocks[0]);
        assert!(!ptr::eq(s, t) && h.is_empty());
        assert_eq!(unsafe { (*s).state() }, SpanState::Full);
        assert_eq!(unsafe { (*t).state() }, SpanState::Cur);
        let r = alloc(&mut other, C64);
        let rm = meta_of(r);
        // the row, oldest first: SPILL_COUNT of s that stay, then
        // the SPILL_COUNT that a spill takes, pushed so the chain
        // reads  a(s) c(t) b(s) r d(s) ... s  from the head
        let (a, b, c, d) = (s_blocks[0], s_blocks[1], t_blocks[0], s_blocks[2]);
        // the last s block stays in hand, so s never empties
        let stay = &s_blocks[3..3 + SPILL_COUNT];
        let go = &s_blocks[3 + SPILL_COUNT..n - 1];
        assert_eq!(go.len(), SPILL_COUNT - 4);
        for &p in stay.iter().chain(go.iter().rev()) {
            assert!(unsafe { h.push(C64, p) });
        }
        for &p in &[d, r, b, c] {
            assert!(unsafe { h.push(C64, p) });
        }
        assert_eq!(h.row_len(C64), SPILL_AT);
        assert!(!unsafe { h.push(C64, a) }, "a full row refuses");
        unsafe { h.spill(C64, a) };
        // s: a, b and the d run came back in three segments
        assert_eq!(unsafe { (*s).n_free() }, SPILL_COUNT - 1);
        assert_eq!(unsafe { (*s).used() }, n - (SPILL_COUNT - 1));
        assert_eq!(unsafe { (*s).state() }, SpanState::Partial);
        assert_eq!(h.partial[C64], s);
        // t: only c, and it stays the current span
        assert_eq!(unsafe { (*t).n_free() }, 1);
        assert_eq!(unsafe { (*t).used() }, n - 1);
        assert_eq!(unsafe { (*t).state() }, SpanState::Cur);
        assert_eq!(h.cur(C64), t);
        // r went to the other heap's span by the atomic path
        assert_eq!(unsafe { (*rm).used() }, n);
        assert_eq!(unsafe { (*rm).collect_remote() }, 1);
        // the row is its older half, s blocks, terminated
        assert_eq!(h.row_len(C64), SPILL_AT - SPILL_COUNT);
        let row = row_chain(&h, C64);
        assert_eq!(row.len(), SPILL_AT - SPILL_COUNT);
        let mut expect: Vec<_> = stay.to_vec();
        expect.reverse();
        assert_eq!(row, expect);
        // s's free list is walkable and holds exactly the released
        // s blocks, newest segment first
        let base = unsafe { (*Arena::containing(s as *mut u8)).span_base(s) };
        let (fh, fn_) = unsafe { (*s).take_chain(base, class_size(C64)) };
        assert_eq!(fn_, SPILL_COUNT - 1);
        let mut free = Vec::new();
        let mut p = fh;
        while !p.is_null() {
            free.push(p);
            p = unsafe { *link(p) };
        }
        let mut want = vec![d];
        want.extend(go.iter());
        want.push(b);
        want.push(a);
        assert_eq!(free, want);
        let _ = alloc(&mut other, C64);
    }

    /// Spill count of a row oscillating across the threshold: one
    /// spill per SPILL_COUNT + 1 frees past a full row, as the trigger
    /// and the amount a spill moves are unchanged.
    #[test]
    fn spills_once_per_half_row_of_frees_past_full() {
        let mut h = heap();
        let n = slots_per_span(C64);
        let blocks: Vec<_> = (0..4 * n).map(|_| alloc(&mut h, C64)).collect();
        let mut spills = 0;
        for &p in &blocks {
            if !unsafe { h.push(C64, p) } {
                unsafe { h.spill(C64, p) };
                spills += 1;
            }
        }
        // the first SPILL_AT frees fill the row; the next one spills
        // and leaves SPILL_COUNT, and every SPILL_COUNT + 1 after that
        // spill once more
        assert_eq!(spills, (4 * n - SPILL_AT + SPILL_COUNT) / (SPILL_COUNT + 1));
        // free / alloc alternating: the row moves by one and back,
        // so the pair never spills
        let p = alloc(&mut h, C64);
        let len = h.row_len(C64);
        for _ in 0..1000 {
            assert!(unsafe { h.push(C64, p) });
            assert_eq!(h.pop(C64), Some(p));
        }
        assert_eq!(h.row_len(C64), len);
    }

    #[test]
    fn a_class_grows_past_a_million_live_blocks() {
        let mut h = heap();
        const N: usize = 1 << 20;
        let mut last = ptr::null_mut();
        for i in 0..N {
            let p = alloc(&mut h, C64);
            unsafe { ptr::write(p as *mut u64, i as u64) };
            last = p;
        }
        assert_eq!(unsafe { ptr::read(last as *const u64) }, (N - 1) as u64);
        let arenas = with_registry(|r| r.len());
        assert!(arenas >= N / (63 * slots_per_span(C64)), "arenas {arenas}");
    }

    #[test]
    fn every_class_hands_out_aligned_writable_blocks() {
        let mut h = heap();
        for class in 0..crate::size_class::N_CLASSES {
            let size = class_size(class);
            for _ in 0..64 {
                let p = alloc(&mut h, class);
                assert_eq!(p as usize % 16, 0);
                unsafe {
                    for off in 0..size {
                        ptr::write(p.add(off), (off & 0xff) as u8);
                    }
                    assert_eq!(*p.add(size - 1), ((size - 1) & 0xff) as u8);
                }
            }
        }
    }

    #[test]
    fn roundtrips_recycle_through_rows_and_spans_without_corruption() {
        let mut h = heap();
        let mut live: Vec<(usize, *mut u8)> = Vec::new();
        for round in 0..300_000usize {
            let class = round % 9;
            let p = alloc(&mut h, class);
            unsafe { ptr::write(p as *mut u64, 0xdead_beef_0000_0000 | round as u64) };
            live.push((class, p));
            if round % 3 != 2 {
                let (c, q) = live.swap_remove(round % live.len());
                free(&mut h, c, q);
            }
        }
        for (i, &(_, p)) in live.iter().enumerate() {
            let v = unsafe { ptr::read(p as *const u64) };
            assert_eq!(v >> 32, 0xdead_beef, "block {i} overwritten");
        }
        for c in 0..9 {
            assert!(h.row_len(c) <= SPILL_AT, "class {c} row {}", h.row_len(c));
        }
    }
}
