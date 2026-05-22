//! Bounded fixed-size memory pool — single-threaded LIFO free-list for
//! fixed-size struct types.
//!
//! Extracted from the torajs AOT TypeScript runtime (commit 8f754ca:
//! P-PERF.A6 "Promise free-list pool" — promise-await -41% / async-fn-call
//! -36% / promise-then -32% wall-clock vs the previous plain-`malloc`
//! path). The algorithm is a head-only LIFO stack bounded at `CAP`
//! entries; overflow falls back to plain heap free, undershoot falls back
//! to a fresh heap alloc.
//!
//! ## Why bounded
//!
//! Pathological churn (e.g. a long-running daemon that builds + drops
//! Promises forever) would otherwise grow the pool unbounded and pin
//! memory. The bound trades worst-case memory for amortized fast-path
//! allocation; tunable via the `CAP` const generic per call-site.
//!
//! ## Why single-threaded
//!
//! torajs runtime is single-threaded today (matching JS spec's single
//! event-loop model). When threading lands the pool will need TLS or a
//! lock; design that explicit when it ships. Current `FixedPool` is
//! `Send + Sync` only when the caller wraps it (it's NOT itself sync).
//!
//! ## Layout invariant
//!
//! `T` must reserve one pointer-sized field that `FixedPool` uses as the
//! "next" link while parked in the pool. The caller picks the field
//! (e.g. the `Promise.callbacks` slot in torajs) and clears it on
//! `acquire` (before user code reads it) + `release` (the pool writes
//! the link in). This avoids a side-table of bookkeeping pointers; the
//! pool reuses one field of the user struct.
//!
//! This is a low-level pool primitive: it does NOT do construction /
//! destruction / refcounting / drop walks. The caller is responsible for
//! initializing the struct's other fields after `acquire` and tearing
//! them down before `release`.
//!
//! ## Safety
//!
//! All API is `unsafe` — pool returns / consumes raw pointers, caller
//! must ensure:
//!  - the pointer is `*mut T` with valid alignment for `T`
//!  - no other code is concurrently using the pool (single-threaded)
//!  - the chosen "next" field is correctly cleared on `acquire`

#![no_std]

extern crate alloc;

use alloc::alloc::{Layout, alloc, dealloc};
use core::cell::Cell;
use core::marker::PhantomData;
use core::ptr;

/// Bounded LIFO free-list pool of `T` with capacity `CAP`.
///
/// Single-threaded. The pool reuses the `next_offset` byte-offset within
/// `T` as the "next" link while structs are parked in the pool —
/// passed via the `FixedPool::new_with_next_offset` constructor.
pub struct FixedPool<T, const CAP: usize> {
    head: Cell<*mut u8>,
    count: Cell<usize>,
    /// Byte offset of the "next" pointer field inside `T` (used while
    /// parked in the pool).
    next_offset: usize,
    _marker: PhantomData<T>,
}

// Single-threaded by construction; we do NOT impl Sync. Callers that
// need multithreading wrap in `Mutex<FixedPool<...>>`.

impl<T, const CAP: usize> FixedPool<T, CAP> {
    /// Build an empty pool. `next_offset` is the byte offset of the
    /// pointer-sized field within `T` that the pool uses as the "next"
    /// link while a struct is parked. The caller picks this field; the
    /// pool overwrites it on `release` and reads it on `acquire`.
    ///
    /// # Safety
    ///
    /// The caller guarantees that `next_offset .. next_offset + size_of::<*mut u8>()`
    /// is within `T`'s layout and is aligned for `*mut u8`.
    pub const unsafe fn new_with_next_offset(next_offset: usize) -> Self {
        Self {
            head: Cell::new(ptr::null_mut()),
            count: Cell::new(0),
            next_offset,
            _marker: PhantomData,
        }
    }

    /// Acquire a fresh-or-recycled `*mut T`. Returns a freshly heap-
    /// allocated `T` when the pool is empty; pops the LIFO head when not.
    ///
    /// The caller MUST treat the returned memory as uninitialized other
    /// than the bytes the pool itself touches: when the pool returns a
    /// recycled slot, the "next" field at `next_offset` is left as-is;
    /// the caller's job to overwrite it with a sentinel (typically zero
    /// / null) before any user-level code reads it.
    ///
    /// # Safety
    ///
    /// Returned pointer is valid for `size_of::<T>()` bytes and aligned
    /// for `T`. Caller fully owns the slot until the matching `release`.
    pub unsafe fn acquire(&self) -> *mut T {
        let head = self.head.get();
        if head.is_null() {
            // Cold path: pool empty, fresh heap alloc.
            let layout = Layout::new::<T>();
            unsafe { alloc(layout) as *mut T }
        } else {
            // Hot path: pop the LIFO head.
            // Safety: `head` was previously written by `release`, which
            // installs the link at byte-offset `self.next_offset`. We
            // read it back from the same offset.
            let next_ptr = unsafe { head.add(self.next_offset) as *mut *mut u8 };
            let next = unsafe { *next_ptr };
            self.head.set(next);
            self.count.set(self.count.get() - 1);
            head as *mut T
        }
    }

    /// Return `p` to the pool. If the pool already has `CAP` entries,
    /// the struct is freed instead (bound enforcement).
    ///
    /// The caller MUST have torn down `T`'s fields before calling
    /// `release` — the pool DOES NOT call `drop`.
    ///
    /// # Safety
    ///
    /// `p` must have been obtained from this pool's `acquire` (so its
    /// layout / alignment matches) AND no other code holds a reference
    /// to its memory.
    pub unsafe fn release(&self, p: *mut T) {
        if self.count.get() < CAP {
            let head = self.head.get();
            // Safety: `p` was a `*mut T` of layout matching `acquire`'s
            // allocation; the "next" field at byte-offset `next_offset`
            // is within `T` and aligned for `*mut u8` per the caller's
            // promise to `new_with_next_offset`.
            unsafe {
                let next_ptr = (p as *mut u8).add(self.next_offset) as *mut *mut u8;
                *next_ptr = head;
            }
            self.head.set(p as *mut u8);
            self.count.set(self.count.get() + 1);
        } else {
            // Overflow path: bound enforcement, plain free.
            let layout = Layout::new::<T>();
            unsafe { dealloc(p as *mut u8, layout) };
        }
    }

    /// Current pooled-slot count (0..=CAP). Debugging + telemetry.
    pub fn pooled(&self) -> usize {
        self.count.get()
    }

    /// Capacity bound (compile-time constant `CAP`).
    pub fn capacity(&self) -> usize {
        CAP
    }
}

impl<T, const CAP: usize> Drop for FixedPool<T, CAP> {
    fn drop(&mut self) {
        // Free every parked entry. Caller already tore down `T`'s
        // fields when each was released; only the heap block remains.
        let layout = Layout::new::<T>();
        let mut p = self.head.get();
        while !p.is_null() {
            unsafe {
                let next_ptr = p.add(self.next_offset) as *mut *mut u8;
                let next = *next_ptr;
                dealloc(p, layout);
                p = next;
            }
        }
        self.head.set(ptr::null_mut());
        self.count.set(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[repr(C)]
    struct Node {
        payload: u64,
        // Offset 8 in this layout: the "next" link field.
        next: *mut u8,
        more: u64,
    }

    #[test]
    fn acquire_when_empty_returns_fresh_alloc() {
        let pool: FixedPool<Node, 4> = unsafe {
            // SAFETY: `next` is at byte offset 8 within Node, aligned
            // for `*mut u8` (u64 precedes it), inside Node's layout.
            FixedPool::new_with_next_offset(8)
        };
        assert_eq!(pool.pooled(), 0);
        let p = unsafe { pool.acquire() };
        assert!(!p.is_null());
        unsafe {
            (*p).payload = 42;
            (*p).next = ptr::null_mut();
            (*p).more = 99;
            assert_eq!((*p).payload, 42);
            assert_eq!((*p).more, 99);
        }
        // Tear down before release.
        unsafe {
            (*p).payload = 0;
            (*p).next = ptr::null_mut();
            (*p).more = 0;
            pool.release(p);
        }
        assert_eq!(pool.pooled(), 1);
    }

    #[test]
    fn release_then_acquire_returns_same_pointer() {
        let pool: FixedPool<Node, 4> = unsafe { FixedPool::new_with_next_offset(8) };
        let p = unsafe { pool.acquire() };
        let original = p as usize;
        unsafe { pool.release(p) };
        let p2 = unsafe { pool.acquire() };
        assert_eq!(p2 as usize, original, "LIFO returns the just-released slot");
        unsafe { pool.release(p2) };
    }

    #[test]
    fn bound_enforces_capacity() {
        let pool: FixedPool<Node, 2> = unsafe { FixedPool::new_with_next_offset(8) };
        let p1 = unsafe { pool.acquire() };
        let p2 = unsafe { pool.acquire() };
        let p3 = unsafe { pool.acquire() };
        unsafe { pool.release(p1) };
        assert_eq!(pool.pooled(), 1);
        unsafe { pool.release(p2) };
        assert_eq!(pool.pooled(), 2);
        // Third release exceeds CAP=2; struct gets freed, count unchanged.
        unsafe { pool.release(p3) };
        assert_eq!(pool.pooled(), 2);
    }

    #[test]
    fn lifo_order_preserved() {
        let pool: FixedPool<Node, 4> = unsafe { FixedPool::new_with_next_offset(8) };
        let p1 = unsafe { pool.acquire() };
        let p2 = unsafe { pool.acquire() };
        let p3 = unsafe { pool.acquire() };
        // Release in 1, 2, 3 order — LIFO pops 3, 2, 1.
        unsafe {
            pool.release(p1);
            pool.release(p2);
            pool.release(p3);
        }
        let r3 = unsafe { pool.acquire() };
        let r2 = unsafe { pool.acquire() };
        let r1 = unsafe { pool.acquire() };
        assert_eq!(r3 as usize, p3 as usize, "LIFO returns last-released first");
        assert_eq!(r2 as usize, p2 as usize);
        assert_eq!(r1 as usize, p1 as usize);
        unsafe {
            pool.release(r1);
            pool.release(r2);
            pool.release(r3);
        }
    }

    #[test]
    fn drop_frees_parked_entries() {
        // The pool's Drop walks the free-list and frees every parked
        // entry. Hard to assert directly (no leak detector in unit
        // test without miri), but we can at least confirm the count
        // resets and subsequent dealloc isn't a double-free
        // (which miri / asan WOULD catch in a more thorough run).
        let pool: FixedPool<Node, 4> = unsafe { FixedPool::new_with_next_offset(8) };
        let p1 = unsafe { pool.acquire() };
        let p2 = unsafe { pool.acquire() };
        unsafe {
            pool.release(p1);
            pool.release(p2);
        }
        assert_eq!(pool.pooled(), 2);
        drop(pool);
        // No assertion past drop — but if the Drop impl is wrong
        // (e.g. double-free) this test would crash on `cargo test`
        // under default allocator.
    }

    #[test]
    fn next_offset_within_type() {
        // Just a sanity probe: the example offset 8 for `Node` matches
        // `field_offset`.
        let n = Node {
            payload: 0,
            next: ptr::null_mut(),
            more: 0,
        };
        let base = &n as *const Node as usize;
        let next_field = &n.next as *const _ as usize;
        assert_eq!(next_field - base, 8);
        let _ = mem::size_of::<Node>(); // 24 bytes on 64-bit
    }
}
