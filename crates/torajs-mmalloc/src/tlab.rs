//! TLAB — the heap's thread-local front: one row of free blocks per
//! size class, a singly linked stack through the blocks themselves,
//! which every sized allocation pops first and every sized free
//! pushes first, without atomics.
//!
//! ```text
//! alloc(size):  class = bucket_for(size)
//!               TLAB.pop(class) or refill: the class's span hands
//!               over its whole free list in one splice
//! free(ptr, size): class = bucket_for(size)
//!               TLAB.push(class, ptr) or spill: half the row goes
//!               back to the blocks' spans
//! ```
//!
//! A row is `{head, len}`, 16 bytes, one `ldp` / `stp` pair. The rows
//! come first in the struct and their layout is fixed by the constants
//! below: the toolchain inlines pop and push at object-cell sites
//! (`torajs-codegen` `compile::cell`) against the Mach-O symbol
//! `___torajs_core_tlab`, testing the row's length against
//! [`SPILL_AT`] with a single `tbnz`, so a row never holds more than
//! `SPILL_AT` blocks. Behind the rows sits the slow path's state, one
//! current span and one partial list per class and the heap's arenas;
//! `heap.rs` drives it.
//!
//! References: tcmalloc `ThreadCache` (per-class linked free lists
//! with a length), mimalloc `mi_heap_t`, Go `mcache`.

use core::ptr;

use crate::arena::Arena;
use crate::size_class::{N_ALL, N_CLASSES};
use crate::span::SpanMeta;

mod quarantine;

pub use quarantine::UAF_POISON;
use quarantine::quarantine;

/// Row length at which a push spills. A power of two, so the inline
/// template tests one bit; a row's length never exceeds it.
pub const SPILL_AT: usize = 256;

/// Byte offset inside a free block where every free list keeps its
/// link: the rows, the spans' local lists and their atomic lists.
/// Past the block's first word, so a block's header word is left as
/// the program last wrote it.
pub const FREE_LINK_OFF: usize = 8;

/// One class's stack of free blocks.
#[repr(C)]
pub struct Row {
    /// Most recently pushed block, linked at `FREE_LINK_OFF`; null
    /// when empty.
    head: *mut u8,
    /// Blocks on the stack, `<= SPILL_AT`.
    len: usize,
}

/// The heap: rows in front, span state behind. The span state covers
/// every class, small and medium; only the small classes have rows.
#[repr(C)]
pub struct TlabCache {
    /// Offset 0, `TLAB_ROW_STRIDE` bytes per small class.
    rows: [Row; N_CLASSES],
    /// The span each class refills from; null between spans.
    pub(crate) cur: [*mut SpanMeta; N_ALL],
    /// Head of each class's doubly linked list of partial spans.
    pub(crate) partial: [*mut SpanMeta; N_ALL],
    /// Doubly linked list of this heap's arenas that have free pages.
    pub(crate) arenas: *mut Arena,
    /// One empty arena kept back from the kernel, so a program that
    /// oscillates around an arena boundary does not map and unmap it
    /// every time.
    pub(crate) spare: *mut Arena,
    /// Empty medium spans held back, newest first (`medium.rs`).
    pub(crate) kept: *mut SpanMeta,
    pub(crate) kept_tail: *mut SpanMeta,
    /// Arena pages the kept spans cover.
    pub(crate) kept_pages: usize,
}

/// Byte offset of `rows`: the first field.
pub const TLAB_ROWS_OFFSET: usize = 0;

/// Bytes per row.
pub const TLAB_ROW_STRIDE: usize = size_of::<Row>();

/// Byte offset of `len` inside a row; `head` is at 0.
pub const TLAB_ROW_LEN_OFF: usize = size_of::<*mut u8>();

const _: () = assert!(TLAB_ROW_STRIDE == 16);
const _: () = assert!(SPILL_AT.is_power_of_two());
// every row is one ldp / stp signed offset from the cache
const _: () = assert!((N_CLASSES - 1) * TLAB_ROW_STRIDE <= 504);

/// A row held outside the cache by [`TlabCache::lift`]: a loop that
/// frees many blocks of one class pushes here without reloading and
/// storing the row around every link write.
pub struct LiftedRow {
    class: usize,
    head: *mut u8,
    len: usize,
}

impl LiftedRow {
    /// [`TlabCache::push`] on the lifted row; `false` when it is at
    /// `SPILL_AT`.
    ///
    /// # Safety
    /// `p` is a live block of the row's class: its link word is written.
    #[inline(always)]
    pub unsafe fn push(&mut self, p: *mut u8) -> bool {
        if self.len & SPILL_AT != 0 {
            return false;
        }
        if unsafe { quarantine(p) } {
            return true;
        }
        #[cfg(torajs_alloc_guard)]
        {
            let mut q = self.head;
            while !q.is_null() {
                assert!(q != p, "block freed twice (row)");
                q = unsafe { *link(q) };
            }
        }
        unsafe { *link(p) = self.head };
        self.head = p;
        self.len += 1;
        true
    }
}

#[inline]
unsafe fn link(p: *mut u8) -> *mut *mut u8 {
    unsafe { p.add(FREE_LINK_OFF) as *mut *mut u8 }
}

impl Default for TlabCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TlabCache {
    pub const fn new() -> Self {
        TlabCache {
            rows: [const {
                Row {
                    head: ptr::null_mut(),
                    len: 0,
                }
            }; N_CLASSES],
            cur: [ptr::null_mut(); N_ALL],
            partial: [ptr::null_mut(); N_ALL],
            arenas: ptr::null_mut(),
            spare: ptr::null_mut(),
            kept: ptr::null_mut(),
            kept_tail: ptr::null_mut(),
            kept_pages: 0,
        }
    }

    /// Pop the row of `class`; `None` on an empty row.
    // Every class index below is `get`, never `[]`: the callers pass
    // a `bucket_for` answer, but the compiler cannot see it, and a
    // `[]` links `panic_bounds_check` and through it the integer
    // formatter, 5 KB of `core` text every program would carry for
    // the allocator alone. An out-of-range index answers "miss".
    #[inline(always)]
    pub fn pop(&mut self, class: usize) -> Option<*mut u8> {
        let row = self.rows.get_mut(class)?;
        let p = row.head;
        if p.is_null() {
            return None;
        }
        row.head = unsafe { *link(p) };
        row.len -= 1;
        Some(p)
    }

    /// Push a block onto the row of `class`; `false` when the row is
    /// at `SPILL_AT`.
    ///
    /// # Safety
    /// `p` is a live block of `class`: its link word is written.
    #[inline(always)]
    pub unsafe fn push(&mut self, class: usize, p: *mut u8) -> bool {
        let Some(row) = self.rows.get_mut(class) else {
            return false;
        };
        if row.len & SPILL_AT != 0 {
            return false;
        }
        if unsafe { quarantine(p) } {
            return true;
        }
        #[cfg(torajs_alloc_guard)]
        {
            let mut q = row.head;
            while !q.is_null() {
                assert!(q != p, "block freed twice (row)");
                q = unsafe { *link(q) };
            }
        }
        unsafe { *link(p) = row.head };
        row.head = p;
        row.len += 1;
        true
    }

    /// Take the row of `class` out for a run of pushes that keeps its
    /// head and length in registers; `None` out of range. Until
    /// [`TlabCache::lower`] puts it back, nothing else may touch this
    /// row: an allocation or free of the class would be lost.
    #[inline(always)]
    pub fn lift(&self, class: usize) -> Option<LiftedRow> {
        let row = self.rows.get(class)?;
        Some(LiftedRow {
            class,
            head: row.head,
            len: row.len,
        })
    }

    /// Put a lifted row back.
    #[inline(always)]
    pub fn lower(&mut self, lifted: &LiftedRow) {
        self.splice(lifted.class, lifted.head, lifted.len);
    }

    /// Lift again the row `lifted` was taken from, once it is back.
    #[inline(always)]
    pub fn relift(&self, lifted: &LiftedRow) -> LiftedRow {
        let (head, len) = self.row(lifted.class);
        LiftedRow {
            class: lifted.class,
            head,
            len,
        }
    }

    /// Make a chain of `len` blocks the row of `class`, replacing
    /// whatever the row held. `len <= SPILL_AT`.
    pub(crate) fn splice(&mut self, class: usize, head: *mut u8, len: usize) {
        if let Some(row) = self.rows.get_mut(class) {
            row.head = head;
            row.len = len;
        }
    }

    /// The row of `class` as `(head, len)`; `(null, 0)` out of range.
    #[inline]
    pub(crate) fn row(&self, class: usize) -> (*mut u8, usize) {
        match self.rows.get(class) {
            Some(r) => (r.head, r.len),
            None => (ptr::null_mut(), 0),
        }
    }

    /// Blocks on the row of `class`.
    #[inline]
    pub fn row_len(&self, class: usize) -> usize {
        match self.rows.get(class) {
            Some(r) => r.len,
            None => 0,
        }
    }

    /// Cached blocks across all classes. Diagnostic.
    pub fn total_cached(&self) -> usize {
        self.rows.iter().map(|r| r.len).sum()
    }

    /// True iff every row is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|r| r.head.is_null())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 16;

    /// Real blocks: a push writes the link word into the block.
    fn blocks(n: usize) -> (Vec<u8>, Vec<*mut u8>) {
        let mut buf = vec![0u8; BLOCK * n];
        let ptrs = (0..n)
            .map(|i| unsafe { buf.as_mut_ptr().add(i * BLOCK) })
            .collect();
        (buf, ptrs)
    }

    #[test]
    fn fresh_tlab_is_empty() {
        let t = TlabCache::new();
        assert!(t.is_empty());
        assert_eq!(t.total_cached(), 0);
    }

    #[test]
    fn pop_on_empty_returns_none() {
        let mut t = TlabCache::new();
        for c in 0..N_CLASSES {
            assert_eq!(t.pop(c), None, "class {} pop on empty", c);
        }
    }

    #[test]
    fn push_then_pop_lifo() {
        let (_buf, p) = blocks(3);
        let mut t = TlabCache::new();
        assert!(unsafe { t.push(2, p[0]) });
        assert!(unsafe { t.push(2, p[1]) });
        assert!(unsafe { t.push(2, p[2]) });
        assert_eq!(t.total_cached(), 3);
        assert_eq!(t.row_len(2), 3);
        assert_eq!(t.pop(2), Some(p[2]));
        assert_eq!(t.pop(2), Some(p[1]));
        assert_eq!(t.pop(2), Some(p[0]));
        assert_eq!(t.pop(2), None);
        assert!(t.is_empty());
    }

    #[test]
    fn a_lifted_row_lands_where_the_row_would() {
        let (_buf, p) = blocks(SPILL_AT + 1);
        let mut t = TlabCache::new();
        assert!(unsafe { t.push(1, p[0]) });
        let mut row = t.lift(1).unwrap();
        assert!(unsafe { row.push(p[1]) });
        t.lower(&row);
        assert_eq!(t.row_len(1), 2);
        // a push made while the row is back in the cache is seen again
        assert!(unsafe { t.push(1, p[2]) });
        let mut row = t.relift(&row);
        for &b in &p[3..SPILL_AT] {
            assert!(unsafe { row.push(b) });
        }
        assert!(!unsafe { row.push(p[SPILL_AT]) }, "full at SPILL_AT");
        t.lower(&row);
        assert_eq!(t.pop(1), Some(p[SPILL_AT - 1]));
        assert_eq!(t.row_len(1), SPILL_AT - 1);
        assert!(t.lift(N_CLASSES).is_none());
    }

    #[test]
    fn push_refuses_at_spill_at() {
        let (_buf, p) = blocks(SPILL_AT + 1);
        let mut t = TlabCache::new();
        for (i, &b) in p.iter().take(SPILL_AT).enumerate() {
            assert!(unsafe { t.push(0, b) }, "push {} fits", i);
        }
        assert!(
            !unsafe { t.push(0, p[SPILL_AT]) },
            "push at SPILL_AT must refuse"
        );
        assert_eq!(t.total_cached(), SPILL_AT);
        // one pop makes room for one push
        assert_eq!(t.pop(0), Some(p[SPILL_AT - 1]));
        assert!(unsafe { t.push(0, p[SPILL_AT]) });
        assert_eq!(t.row_len(0), SPILL_AT);
    }

    #[test]
    fn classes_are_independent() {
        let (_buf, p) = blocks(3);
        let mut t = TlabCache::new();
        assert!(unsafe { t.push(0, p[0]) });
        assert!(unsafe { t.push(5, p[1]) });
        assert!(unsafe { t.push(8, p[2]) });
        assert_eq!(t.pop(0), Some(p[0]));
        assert_eq!(t.pop(5), Some(p[1]));
        assert_eq!(t.pop(8), Some(p[2]));
        assert!(t.is_empty());
    }

    #[test]
    fn splice_installs_a_chain_the_pops_walk() {
        let (_buf, p) = blocks(4);
        unsafe {
            *link(p[0]) = p[1];
            *link(p[1]) = p[2];
            *link(p[2]) = p[3];
            *link(p[3]) = ptr::null_mut();
        }
        let mut t = TlabCache::new();
        t.splice(3, p[0], 4);
        assert_eq!(t.row_len(3), 4);
        for &b in &p {
            assert_eq!(t.pop(3), Some(b));
        }
        assert_eq!(t.pop(3), None);
        assert_eq!(t.row_len(3), 0);
    }

    /// The link word sits past the block's first word.
    #[test]
    fn push_writes_only_the_link_word() {
        let mut buf = [0xaau8; BLOCK];
        let mut t = TlabCache::new();
        assert!(unsafe { t.push(0, buf.as_mut_ptr()) });
        assert_eq!(buf[..FREE_LINK_OFF], [0xaa; FREE_LINK_OFF]);
        assert_eq!(
            usize::from_ne_bytes(buf[FREE_LINK_OFF..FREE_LINK_OFF + 8].try_into().unwrap()),
            0
        );
    }

    /// The inline-emit constants must match the real layout: a drift
    /// means emitted pop / push reads the wrong field.
    #[test]
    fn ir_layout_constants_match_actual_struct() {
        let t = TlabCache::new();
        let base = &t as *const _ as usize;
        let rows = &t.rows as *const _ as usize;
        assert_eq!(rows - base, TLAB_ROWS_OFFSET, "TLAB_ROWS_OFFSET drift");
        let r1 = &t.rows[1] as *const _ as usize;
        assert_eq!(r1 - rows, TLAB_ROW_STRIDE, "TLAB_ROW_STRIDE drift");
        let len1 = &t.rows[1].len as *const _ as usize;
        assert_eq!(len1 - r1, TLAB_ROW_LEN_OFF, "TLAB_ROW_LEN_OFF drift");
        // the span state sits behind the rows, where the template
        // never looks
        let cur_addr = &t.cur as *const _ as usize;
        assert!(cur_addr - base >= N_CLASSES * TLAB_ROW_STRIDE);
    }

    /// Every free block has room for the link word.
    #[test]
    fn free_link_fits_the_smallest_class() {
        assert!(FREE_LINK_OFF + size_of::<*mut u8>() <= crate::size_class::SIZE_CLASSES[0]);
    }
}
