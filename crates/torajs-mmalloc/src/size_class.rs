//! Size-class allocator built on [`Span`].
//!
//! Phase 2a item 2 of the metal-tier redesign — span-backed
//! replacement for the prior PageBump + per-class freelist shape.
//! Each size class owns a growable span array ([`SpanVec`]).
//! Allocation walks spans LIFO (most-recently-grown first) hunting
//! for a slot; on miss across all open spans, a fresh span is
//! mmap'd via [`Span::new_for_class`] and appended.
//!
//! ## No per-class cap (chunk 633)
//!
//! The pre-633 shape was `[[Option<Span>; 4096]; 9]` — a
//! compile-time 64 MB-per-class arena ceiling. Programs with ~2^20
//! live 64-byte allocations (probe l12g: 300k live WeakMap keys →
//! 4×40B allocs/iter) hit the cap, `alloc` answered `None`, and
//! the NULL propagated into runtime writes — SIGSEGV. Spans are now
//! tracked in a mmap-backed growable array (Go `mheap` / mimalloc
//! segment-list shape): the only allocation ceiling is physical
//! memory. Span metadata moves are raw byte copies — [`Span`]'s
//! munmap `Drop` must never run for a moved element.
//!
//! ## Full-prefix watermark
//!
//! `first_open[class]` tracks the lowest span index that might have
//! a free slot. The AOT runtime never returns slots to a span
//! (frees ride the TLAB / CentralQueue cycle), so bump-full spans
//! below the watermark are permanently full and the LIFO scan skips
//! them — pure-growth workloads stay O(1) per alloc instead of
//! rescanning every full span before each grow. `dealloc` pulls the
//! watermark back down when it returns a slot to a lower span.
//!
//! Free is `Span::free_slot` after a same-class `contains(ptr)`
//! scan to dispatch — O(n_spans_in_class); only the Rust-side
//! `GlobalAlloc` (host compiler process) reaches it.
//!
//! Public surface is invariant vs the pre-Phase-2a shape:
//! `Allocator::new` / `alloc(size)` / `dealloc(ptr, size)` /
//! `bucket_for(size)` / `mapped_bytes()`.

use core::mem::size_of;
use core::ptr;

use crate::large::page_round_up;
use crate::span::{SPAN_LEN, Span};
use torajs_syscall::{mmap_anon_rw, munmap};

/// Power-of-two size classes covered by the per-class span pool.
/// Requests larger than the last entry route to `super::large`.
pub const SIZE_CLASSES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// First `SpanVec` growth — 64 spans × 32 B = one 4 KB page's worth
/// of metadata (page-rounded by the mmap anyway).
const SPANVEC_INITIAL_CAP: usize = 64;

/// Growable span array, mmap-backed (no allocator recursion — the
/// backing comes straight from the kernel, same as [`Span`]s
/// themselves). Element moves during growth are raw byte copies;
/// the old backing is munmap'd without dropping elements so a
/// moved [`Span`]'s `Drop` (munmap of its 16 KB region) never
/// fires for a live span.
struct SpanVec {
    ptr: *mut Span,
    cap: usize,
    len: usize,
}

impl SpanVec {
    const fn new() -> Self {
        SpanVec {
            ptr: ptr::null_mut(),
            cap: 0,
            len: 0,
        }
    }

    /// Ensure room for one more span. Doubles the mmap'd backing on
    /// full; `false` on kernel mmap failure (true OOM).
    fn reserve_one(&mut self) -> bool {
        if self.len < self.cap {
            return true;
        }
        let new_cap = if self.cap == 0 {
            SPANVEC_INITIAL_CAP
        } else {
            self.cap * 2
        };
        let Ok(p) = mmap_anon_rw(page_round_up(new_cap * size_of::<Span>())) else {
            return false;
        };
        let new_ptr = p as *mut Span;
        if self.len > 0 {
            // Raw byte move — see struct doc (no element Drop).
            unsafe { ptr::copy_nonoverlapping(self.ptr, new_ptr, self.len) };
        }
        if !self.ptr.is_null() {
            let _ = unsafe {
                munmap(
                    self.ptr as *mut u8,
                    page_round_up(self.cap * size_of::<Span>()),
                )
            };
        }
        self.ptr = new_ptr;
        self.cap = new_cap;
        true
    }

    /// Append. Caller must have `reserve_one`'d.
    fn push(&mut self, s: Span) {
        debug_assert!(self.len < self.cap);
        unsafe { ptr::write(self.ptr.add(self.len), s) };
        self.len += 1;
    }

    #[inline]
    fn get_mut(&mut self, i: usize) -> &mut Span {
        debug_assert!(i < self.len);
        unsafe { &mut *self.ptr.add(i) }
    }
}

impl Drop for SpanVec {
    fn drop(&mut self) {
        // Test-process hygiene only — the production instance lives
        // in a `static mut` and never drops. Drop each live span
        // (munmaps its region), then the backing array.
        for i in 0..self.len {
            unsafe { ptr::drop_in_place(self.ptr.add(i)) };
        }
        if !self.ptr.is_null() {
            let _ = unsafe {
                munmap(
                    self.ptr as *mut u8,
                    page_round_up(self.cap * size_of::<Span>()),
                )
            };
        }
    }
}

pub struct Allocator {
    /// Per-class growable span pool. Spans append monotonically;
    /// index order = growth order.
    classes: [SpanVec; SIZE_CLASSES.len()],
    /// Full-prefix watermark per class (see module doc): every span
    /// at index `< first_open` is known-full. Alloc scans
    /// `[first_open, len)` LIFO; dealloc pulls the mark back down.
    first_open: [usize; SIZE_CLASSES.len()],
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub const fn new() -> Self {
        Allocator {
            classes: [const { SpanVec::new() }; SIZE_CLASSES.len()],
            first_open: [0; SIZE_CLASSES.len()],
        }
    }

    /// Round `size` up to the next size class index; returns
    /// `None` if `size` exceeds the largest bucket.
    ///
    /// Rotation 470 — this used to walk `SIZE_CLASSES` looking for
    /// the first one that fits, and every alloc AND every free pays
    /// it (`core::alloc_sized` / `core::free_sized`). The classes are
    /// consecutive powers of two starting at 16, so the answer is
    /// `ceil_log2(size) - 4` clamped at zero — two instructions, no
    /// loop and no branch on the class count. `size_classes_are_the
    /// _powers_of_two_this_assumes` pins the assumption and
    /// `bucket_for_matches_a_linear_scan` checks every size across
    /// the whole range against the walk this replaces.
    #[inline]
    pub fn bucket_for(size: usize) -> Option<usize> {
        if size > SIZE_CLASSES[SIZE_CLASSES.len() - 1] {
            return None;
        }
        let ceil_log2 = usize::BITS - size.saturating_sub(1).leading_zeros();
        Some((ceil_log2 as usize).saturating_sub(4))
    }

    /// Allocate `size` bytes from the appropriate size-class pool.
    /// Returns `None` on kernel mmap failure (true OOM) or `size`
    /// past the largest class (caller routes to `super::large`).
    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        let bucket = Self::bucket_for(size)?;
        // `bucket_for` answers below the class count by construction,
        // but the compiler cannot see it, and a `[]` here would link
        // `panic_bounds_check` — and through it `Display for usize`
        // and `Formatter::pad_integral`, 5 KB of `core` text in every
        // program (r502: the empty program's only edge into it). The
        // compare-and-branch is the same one the check made; only
        // the panic call is gone. Out of range answers "not mine".
        let class_size = *SIZE_CLASSES.get(bucket)?;
        let first_open = *self.first_open.get(bucket)?;

        // 1. LIFO span scan over the open range — newest span first
        //    (the bump span; common case hits on the first probe).
        //    A full sweep with no hit proves every open span is
        //    full: advance the watermark past them.
        let list = self.classes.get_mut(bucket)?;
        for i in (first_open..list.len).rev() {
            if let Some(p) = list.get_mut(i).alloc_slot() {
                return Some(p);
            }
        }
        if let Some(fo) = self.first_open.get_mut(bucket) {
            *fo = list.len;
        }

        // 2. All open spans full — grow (no cap; kernel is the
        //    ceiling).
        if !list.reserve_one() {
            return None;
        }
        let mut new_span = Span::new_for_class(class_size, bucket as u8).ok()?;
        let p = new_span.alloc_slot()?;
        list.push(new_span);
        Some(p)
    }

    /// Release a previously-allocated block. `size` must be the
    /// SAME value passed to `alloc` (size-class allocator has no
    /// per-block size metadata in this API; caller bookkeeping
    /// required — `super::extern_api` Layer 2 shim handles this
    /// via SHIM_HEADER for libc-compat consumers).
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by `alloc(size)`, and not
    /// already freed (double-free is UB and will corrupt the
    /// owning span's freelist).
    pub unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize) {
        let Some(bucket) = Self::bucket_for(size) else {
            // Out of bucket range — caller should have used
            // large_alloc/large_free; silently leak to keep the
            // invariant simple.
            return;
        };
        // Dispatch ptr to its owning span: same-class scan. `get`
        // for the same reason as `alloc`.
        let Some(list) = self.classes.get_mut(bucket) else {
            return;
        };
        for i in 0..list.len {
            let span = list.get_mut(i);
            if span.contains(ptr) {
                // SAFETY: ptr is contained in this span; caller's
                // outer Safety invariant says ptr was from a
                // matching `alloc(size)`, which placed it in
                // exactly this size class.
                unsafe { span.free_slot(ptr) };
                // The span has a free slot again — reopen it for
                // the alloc scan.
                if let Some(fo) = self.first_open.get_mut(bucket)
                    && i < *fo
                {
                    *fo = i;
                }
                return;
            }
        }
        // ptr not in any span — silently drop (matches legacy
        // behavior: mis-sized free is leak, not UB).
    }

    /// Total bytes addressable from the kernel via this allocator.
    /// = `sum over classes of (active_spans × SPAN_LEN)`.
    /// Diagnostic, not a runtime hot-path.
    pub fn mapped_bytes(&self) -> usize {
        let mut sum = 0usize;
        for list in self.classes.iter() {
            sum += list.len * SPAN_LEN;
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    #[test]
    fn alloc_and_free_recycles() {
        let mut a = Allocator::new();
        let p1 = a.alloc(16).expect("alloc 16");
        unsafe { *p1 = 0xab };
        unsafe { a.dealloc(p1, 16) };
        // Next alloc of same bucket should hand back the same
        // block (Span freelist is LIFO).
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

    /// The bit-math `bucket_for` is only correct while the classes
    /// are consecutive powers of two starting at 16.
    #[test]
    fn size_classes_are_the_powers_of_two_this_assumes() {
        for (i, &c) in SIZE_CLASSES.iter().enumerate() {
            assert_eq!(c, 16usize << i, "class {i}");
        }
    }

    #[test]
    fn bucket_for_matches_a_linear_scan() {
        let linear = |size: usize| -> Option<usize> {
            if size == 0 {
                return Some(0);
            }
            SIZE_CLASSES.iter().position(|&c| size <= c)
        };
        for size in 0..=(SIZE_CLASSES[SIZE_CLASSES.len() - 1] + 64) {
            assert_eq!(Allocator::bucket_for(size), linear(size), "size {size}");
        }
    }

    #[test]
    fn cross_span_alloc() {
        let mut a = Allocator::new();
        // Fill span 1 with 256-class allocations: 16 KB / 256 = 64
        // slots fits exactly; the 65th should trigger a new span.
        for _ in 0..64 {
            let p = a.alloc(256).expect("alloc 256");
            unsafe { *p = 0xcd };
        }
        let p = a.alloc(256).expect("alloc 256 across spans");
        unsafe { *p = 0xef };
        assert_eq!(a.mapped_bytes(), 2 * SPAN_LEN);
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

    /// Every alloc returns a 16-byte aligned pointer (matches macOS
    /// libc malloc guarantee). SIZE_CLASSES are multiples of 16
    /// and SPAN_LEN is 16K, so the invariant holds by construction —
    /// this test pins it down so future cursor edits in Span can't
    /// silently break alignment for SIMD / `_Atomic` heap reads.
    #[test]
    fn alloc_pointers_are_16_byte_aligned() {
        let mut a = Allocator::new();
        for _ in 0..1024 {
            for &size in SIZE_CLASSES.iter() {
                let p = a.alloc(size).expect("alloc") as usize;
                assert_eq!(
                    p & 0xf,
                    0,
                    "alloc({}) returned 0x{:x} (not 16-byte aligned)",
                    size,
                    p
                );
            }
        }
    }

    /// Stress: 100K alloc/free roundtrips across all size classes
    /// without corruption. Catches freelist double-link / cross-
    /// span dispatch bugs that single-shot tests miss.
    #[test]
    fn stress_100k_roundtrips_no_corruption() {
        let mut a = Allocator::new();
        let sizes = [16usize, 32, 64, 128, 256, 512, 1024, 2048, 4096];
        for round in 0..100_000usize {
            let size = sizes[round % sizes.len()];
            let p = a.alloc(size).expect("alloc");
            unsafe {
                let header = p as *mut u64;
                let magic = 0xdeadbeef00000000u64 | (round as u64);
                ptr::write(header, magic);
                assert_eq!(ptr::read(header), magic);
                a.dealloc(p, size);
            }
        }
    }

    /// Span-backed shape regression: verify two allocs in the same
    /// class share a single span until that span is full. (Legacy
    /// PageBump shape could mix-size within a page; new Span shape
    /// must NOT.)
    #[test]
    fn same_class_packs_into_one_span() {
        let mut a = Allocator::new();
        let p1 = a.alloc(64).expect("alloc 1");
        let p2 = a.alloc(64).expect("alloc 2");
        // Both should be in the same span: addresses differ by
        // slot_size (64 B), not by span_len.
        let delta = (p2 as usize).abs_diff(p1 as usize);
        assert_eq!(delta, 64, "same-class allocs not packed in one span");
        assert_eq!(a.mapped_bytes(), SPAN_LEN, "should be exactly 1 span");
    }

    /// Chunk 633 — the old `[Option<Span>; 4096]` shape NULL'd out
    /// past 2^20 live 64-byte slots (probe l12g SIGSEGV). Push a
    /// class well past its former 4096-span cap and verify every
    /// alloc lands. 5000 spans × 256 slots = 1.28M live allocs.
    #[test]
    fn class_grows_past_former_4096_span_cap() {
        let mut a = Allocator::new();
        const SPANS: usize = 5000;
        const PER_SPAN: usize = SPAN_LEN / 64;
        for i in 0..(SPANS * PER_SPAN) {
            let p = a.alloc(64).unwrap_or(core::ptr::null_mut());
            assert!(!p.is_null(), "alloc {} returned NULL (former cap wall)", i);
            // Touch the slot — catches metadata-move corruption.
            unsafe { ptr::write(p as *mut u64, i as u64) };
        }
        assert_eq!(a.mapped_bytes(), SPANS * SPAN_LEN);
    }

    /// Watermark regression: dealloc into a low (full) span reopens
    /// it for the alloc scan.
    #[test]
    fn dealloc_reopens_full_span_below_watermark() {
        let mut a = Allocator::new();
        let per_span = SPAN_LEN / 4096; // 4 slots
        let mut first_span_ptrs = vec![];
        for i in 0..(per_span * 3) {
            let p = a.alloc(4096).expect("alloc");
            if i < per_span {
                first_span_ptrs.push(p);
            }
        }
        // Three spans, first two full + third full → watermark will
        // pass them on the next miss-sweep. Free a slot in span 0.
        unsafe { a.dealloc(first_span_ptrs[0], 4096) };
        let p = a.alloc(4096).expect("realloc");
        assert_eq!(p, first_span_ptrs[0], "freed low-span slot not reused");
        assert_eq!(a.mapped_bytes(), 3 * SPAN_LEN, "should not have grown");
    }
}
