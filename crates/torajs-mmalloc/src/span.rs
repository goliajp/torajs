//! `SpanMeta` — the bookkeeping of one span: a run of arena pages
//! carved into equal slots of one size class.
//!
//! The metadata lives in the arena header, never inside the span, so
//! a freed block's bytes are the only thing a free list writes: the
//! link word at [`FREE_LINK_OFF`], past the block's first word.
//!
//! Two free lists. `free` belongs to the owning heap, is read and
//! written without atomics, and carries its length so the heap can
//! take the whole chain in one splice; `thread_free` is where another
//! thread's heap pushes a block it released, with a compare-and-swap,
//! and the owner takes the whole list in one exchange when it refills.
//!
//! `used` counts the slots the owner carved out and has not yet taken
//! back onto `free`: blocks in the user's hands, blocks on the owner's
//! row, and blocks on `thread_free` that the owner has not collected.
//! Only the owner touches it, so a span is known empty exactly when
//! the owner sees `used == 0`.
//!
//! Shape after mimalloc `mi_page_t` (local + thread free list, `used`)
//! and Go `mspan` (metadata off-span, state machine).

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::tlab::{FREE_LINK_OFF, SPILL_AT};

/// Virgin slots a refill carves into a chain at once: enough that a
/// program growing a class pays the carve per 64 blocks, not more
/// than a row's spill returns.
const CARVE: usize = 64;

/// Where a span is, as the owner's lists see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum SpanState {
    /// Pages back in the arena's bitmap; the metadata slot is reusable.
    Free = 0,
    /// The span the owner currently refills from for its class.
    Cur = 1,
    /// Has free slots and sits in the owner's partial list for its class.
    Partial = 2,
    /// Every slot carved and none on `free`; in no list.
    Full = 3,
}

/// Per-span bookkeeping, 48 bytes, kept in the arena header.
#[repr(C)]
pub struct SpanMeta {
    /// Owner-local free list head; links at `FREE_LINK_OFF`.
    free: *mut u8,
    /// Blocks released by other threads, pushed with a CAS.
    thread_free: AtomicPtr<u8>,
    /// Partial-list links.
    pub next: *mut SpanMeta,
    pub prev: *mut SpanMeta,
    /// Slots carved so far from the span's virgin tail.
    bump: u16,
    /// Total slots in the span.
    slot_count: u16,
    /// Carved slots not on `free`; see the module doc.
    used: u16,
    /// Blocks on `free`.
    n_free: u16,
    /// Size class index.
    class: u8,
    /// Arena pages the span covers.
    pages: u8,
    state: SpanState,
    _pad: [u8; 5],
}

const _: () = assert!(size_of::<SpanMeta>() == 48);

#[inline]
unsafe fn link(p: *mut u8) -> *mut *mut u8 {
    unsafe { p.add(FREE_LINK_OFF) as *mut *mut u8 }
}

impl SpanMeta {
    /// Begin a span of `slot_count` slots of class `class` over
    /// `pages` pages, with nothing carved yet.
    pub fn init(&mut self, class: usize, pages: usize, slot_count: usize) {
        self.free = ptr::null_mut();
        self.thread_free.store(ptr::null_mut(), Ordering::Relaxed);
        self.next = ptr::null_mut();
        self.prev = ptr::null_mut();
        self.bump = 0;
        self.slot_count = slot_count as u16;
        self.used = 0;
        self.n_free = 0;
        self.class = class as u8;
        self.pages = pages as u8;
        self.state = SpanState::Cur;
    }

    #[inline]
    pub fn class(&self) -> usize {
        self.class as usize
    }

    #[inline]
    pub fn pages(&self) -> usize {
        self.pages as usize
    }

    #[inline]
    pub fn state(&self) -> SpanState {
        self.state
    }

    #[inline]
    pub fn set_state(&mut self, s: SpanState) {
        self.state = s;
    }

    #[inline]
    pub fn used(&self) -> usize {
        self.used as usize
    }

    #[inline]
    pub fn n_free(&self) -> usize {
        self.n_free as usize
    }

    #[inline]
    pub fn slot_count(&self) -> usize {
        self.slot_count as usize
    }

    /// Slots the span can still hand out without a refill from
    /// another thread: on `free` plus never carved.
    #[inline]
    pub fn has_local(&self) -> bool {
        self.n_free != 0 || self.bump < self.slot_count
    }

    /// Detach a chain of slots for the owner's row: the whole local
    /// free list when it fits a row, its first `SPILL_AT` blocks when
    /// it does not, else up to `CARVE` virgin slots linked together.
    /// Returns `(head, len)`; `len == 0` means the span has nothing
    /// of its own left.
    ///
    /// # Safety
    /// `base` is the span's first byte and `slot_size` its class size.
    pub unsafe fn take_chain(&mut self, base: *mut u8, slot_size: usize) -> (*mut u8, usize) {
        let n_free = self.n_free as usize;
        if n_free != 0 {
            let head = self.free;
            let n = if n_free <= SPILL_AT {
                self.free = ptr::null_mut();
                n_free
            } else {
                // cut after the first SPILL_AT blocks
                let mut last = head;
                for _ in 1..SPILL_AT {
                    last = unsafe { *link(last) };
                }
                unsafe {
                    self.free = *link(last);
                    *link(last) = ptr::null_mut();
                }
                SPILL_AT
            };
            self.n_free -= n as u16;
            self.used += n as u16;
            return (head, n);
        }
        let left = (self.slot_count - self.bump) as usize;
        if left == 0 {
            return (ptr::null_mut(), 0);
        }
        let n = left.min(CARVE);
        let first = unsafe { base.add(self.bump as usize * slot_size) };
        let mut p = first;
        for _ in 1..n {
            let next = unsafe { p.add(slot_size) };
            unsafe { *link(p) = next };
            p = next;
        }
        unsafe { *link(p) = ptr::null_mut() };
        self.bump += n as u16;
        self.used += n as u16;
        (first, n)
    }

    /// Owner path: link a block of this span onto `free`. The caller
    /// settles `used` for the whole run with [`SpanMeta::take_back`].
    ///
    /// # Safety
    /// `p` is a block carved from this span, not already on a list.
    #[inline]
    pub unsafe fn push_free(&mut self, p: *mut u8) {
        #[cfg(torajs_alloc_guard)]
        {
            let mut q = self.free;
            while !q.is_null() {
                assert!(q != p, "block freed twice (span)");
                q = unsafe { *link(q) };
            }
        }
        unsafe { *link(p) = self.free };
        self.free = p;
        self.n_free += 1;
    }

    /// `n` blocks are back on `free`.
    #[inline]
    pub fn take_back(&mut self, n: usize) {
        #[cfg(torajs_alloc_guard)]
        assert!(
            self.used as usize >= n,
            "span takes back more than it gave out"
        );
        self.used -= n as u16;
    }

    /// Foreign path: another heap pushes a block of this span.
    ///
    /// # Safety
    /// As [`SpanMeta::push_free`].
    pub unsafe fn release_remote(&self, p: *mut u8) {
        let mut cur = self.thread_free.load(Ordering::Acquire);
        loop {
            unsafe { *link(p) = cur };
            match self.thread_free.compare_exchange_weak(
                cur,
                p,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(now) => cur = now,
            }
        }
    }

    /// Owner path: move everything on `thread_free` onto `free`.
    /// Returns how many blocks came back.
    pub fn collect_remote(&mut self) -> usize {
        let head = self.thread_free.swap(ptr::null_mut(), Ordering::Acquire);
        if head.is_null() {
            return 0;
        }
        let mut n = 1;
        let mut tail = head;
        loop {
            let next = unsafe { *link(tail) };
            if next.is_null() {
                break;
            }
            tail = next;
            n += 1;
        }
        unsafe { *link(tail) = self.free };
        self.free = head;
        self.n_free += n as u16;
        self.used -= n as u16;
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    const SLOT: usize = 64;
    const SLOTS: usize = 8;

    fn meta(slots: usize) -> SpanMeta {
        let mut m: MaybeUninit<SpanMeta> = MaybeUninit::zeroed();
        unsafe { m.assume_init_mut() }.init(2, 1, slots);
        unsafe { m.assume_init() }
    }

    /// Walk a chain, collecting its blocks.
    fn chain(head: *mut u8) -> Vec<*mut u8> {
        let mut v = Vec::new();
        let mut p = head;
        while !p.is_null() {
            v.push(p);
            p = unsafe { *link(p) };
        }
        v
    }

    unsafe fn release(m: &mut SpanMeta, p: *mut u8) {
        unsafe { m.push_free(p) };
        m.take_back(1);
    }

    #[test]
    fn take_chain_carves_the_tail_in_order_then_recycles_lifo() {
        let mut buf = [0u8; SLOT * SLOTS];
        let base = buf.as_mut_ptr();
        let mut m = meta(SLOTS);
        let (head, n) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!(n, SLOTS, "a small span carves whole");
        let blocks = chain(head);
        assert_eq!(blocks.len(), SLOTS);
        for (i, &p) in blocks.iter().enumerate() {
            assert_eq!(p as usize - base as usize, i * SLOT);
        }
        assert_eq!(m.used(), SLOTS);
        assert!(!m.has_local());
        assert_eq!(unsafe { m.take_chain(base, SLOT) }, (ptr::null_mut(), 0));
        unsafe { release(&mut m, blocks[0]) };
        unsafe { release(&mut m, blocks[3]) };
        assert_eq!(m.n_free(), 2);
        assert_eq!(m.used(), SLOTS - 2);
        let (head, n) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!(n, 2);
        assert_eq!(chain(head), vec![blocks[3], blocks[0]]);
        assert_eq!(m.n_free(), 0);
        assert_eq!(m.used(), SLOTS);
    }

    #[test]
    fn take_chain_carves_at_most_carve_slots_at_a_time() {
        const BIG: usize = CARVE * 2 + 5;
        let mut buf = vec![0u8; SLOT * BIG];
        let base = buf.as_mut_ptr();
        let mut m = meta(BIG);
        let (_, n1) = unsafe { m.take_chain(base, SLOT) };
        let (_, n2) = unsafe { m.take_chain(base, SLOT) };
        let (h3, n3) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!((n1, n2, n3), (CARVE, CARVE, 5));
        assert_eq!(chain(h3).len(), 5);
        assert_eq!(m.used(), BIG);
    }

    #[test]
    fn take_chain_cuts_a_long_free_list_at_spill_at() {
        const BIG: usize = SPILL_AT + 40;
        let mut buf = vec![0u8; SLOT * BIG];
        let base = buf.as_mut_ptr();
        let mut m = meta(BIG);
        let mut all = Vec::new();
        loop {
            let (h, n) = unsafe { m.take_chain(base, SLOT) };
            if n == 0 {
                break;
            }
            all.extend(chain(h));
        }
        assert_eq!(all.len(), BIG);
        for &p in &all {
            unsafe { release(&mut m, p) };
        }
        assert_eq!(m.n_free(), BIG);
        let (h, n) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!(n, SPILL_AT);
        let first = chain(h);
        assert_eq!(first.len(), SPILL_AT);
        assert_eq!(first[0], all[BIG - 1], "newest release first");
        assert_eq!(m.n_free(), 40);
        assert_eq!(m.used(), SPILL_AT);
        let (h, n) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!(n, 40);
        assert_eq!(chain(h).len(), 40);
        assert_eq!(m.used(), BIG);
        assert!(!m.has_local());
    }

    /// The link word sits past the block's first word, which keeps
    /// whatever header the block carried.
    #[test]
    fn push_free_writes_only_the_link_word() {
        let mut buf = [0xaau8; SLOT * SLOTS];
        let base = buf.as_mut_ptr();
        let mut m = meta(SLOTS);
        let (h, _) = unsafe { m.take_chain(base, SLOT) };
        let p = chain(h)[0];
        buf.fill(0xaa);
        unsafe { release(&mut m, p) };
        assert_eq!(buf[0..FREE_LINK_OFF], [0xaa; FREE_LINK_OFF]);
        assert_eq!(
            buf[FREE_LINK_OFF + 8..SLOT],
            [0xaa; SLOT - FREE_LINK_OFF - 8]
        );
        let word = usize::from_ne_bytes(buf[FREE_LINK_OFF..FREE_LINK_OFF + 8].try_into().unwrap());
        assert_eq!(word, 0, "first release links to null");
    }

    #[test]
    fn remote_releases_collect_onto_the_local_list() {
        let mut buf = [0u8; SLOT * SLOTS];
        let base = buf.as_mut_ptr();
        let mut m = meta(SLOTS);
        let (h, _) = unsafe { m.take_chain(base, SLOT) };
        let blocks = chain(h);
        let (a, b, c) = (blocks[0], blocks[1], blocks[2]);
        unsafe { m.release_remote(a) };
        unsafe { m.release_remote(b) };
        assert_eq!(m.used(), SLOTS, "remote releases leave used alone");
        unsafe { release(&mut m, c) };
        assert_eq!(m.collect_remote(), 2);
        assert_eq!(m.used(), SLOTS - 3);
        assert_eq!(m.n_free(), 3);
        assert_eq!(m.collect_remote(), 0);
        // collected blocks go in front of the local list
        let (h, n) = unsafe { m.take_chain(base, SLOT) };
        assert_eq!(n, 3);
        assert_eq!(chain(h), vec![b, a, c]);
    }

    #[test]
    fn init_sets_the_geometry_and_state() {
        let m = meta(SLOTS);
        assert_eq!(m.class(), 2);
        assert_eq!(m.pages(), 1);
        assert_eq!(m.slot_count(), SLOTS);
        assert_eq!(m.state(), SpanState::Cur);
        assert!(m.has_local());
    }
}
