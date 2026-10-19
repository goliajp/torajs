//! Array LIFO recycler — small-cap-match thread-local pool.
//!
//! Stores blocks keyed by their capacity so tight loops like
//! `[1,2,3].forEach(...)` (one fresh literal alloc per iteration)
//! turn malloc/free pairs into pop/push.
//!
//! ## Cap-match LIFO
//!
//! Unlike `torajs-str::pool` (single uniform block size class), an
//! array pool entry has a `cap` payload size that the alloc path
//! must match. The pool stores `(block_ptr, cap)` pairs and searches
//! `[0..count)` from the head down on `pop_cap_match`. Tight loops
//! typically hit the LIFO head on the first compare; falling
//! through to libc malloc costs ~30 ns extra.
//!
//! ## Single-threaded by contract, `Atomic*` for safety story
//!
//! Same rationale as `torajs-str::pool` — tora runtime is single-
//! threaded, `static mut` would trip the Rust 2024 `static_mut_refs`
//! lint, `Atomic*` with `Ordering::Relaxed` compiles to the same
//! instructions while keeping the API `&'static` clean.

use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};

/// Max blocks held in the pool. Matches C's `__TORAJS_ARR_POOL_SLOTS`.
pub const POOL_SLOTS: usize = 16;

/// Caps above this bypass the pool (direct malloc). Matches C's
/// `__TORAJS_ARR_POOL_CAP_MAX`.
pub const POOL_CAP_MAX: u64 = 32;

/// Block pointer slots. `BLOCKS[i]` corresponds to `CAPS[i]`.
static BLOCKS: [AtomicPtr<u8>; POOL_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; POOL_SLOTS];

/// Cap-of-each-block. `CAPS[i]` is the user-visible capacity of the
/// block at `BLOCKS[i]`, used by alloc-path cap-match search.
static CAPS: [AtomicU64; POOL_SLOTS] = [const { AtomicU64::new(0) }; POOL_SLOTS];

/// Number of occupied slots — `BLOCKS[0..COUNT]` / `CAPS[0..COUNT]`.
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// Current occupied count, for callers that want to gate on "pool not
/// full" before computing other prerequisites.
#[inline]
pub fn current_count() -> usize {
    COUNT.load(Ordering::Relaxed)
}

/// Find a block with matching cap; if found, pop it out (swap with
/// last, decrement count) and return the pointer. Returns
/// `core::ptr::null_mut()` on miss. Searches LIFO-end-first so tight
/// loops hit the most-recently-released block immediately.
#[inline]
pub fn pop_cap_match(cap: u64) -> *mut u8 {
    let count = COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return ptr::null_mut();
    }
    let mut i = count as isize - 1;
    while i >= 0 {
        let idx = i as usize;
        if CAPS[idx].load(Ordering::Relaxed) == cap {
            let p = BLOCKS[idx].load(Ordering::Relaxed);
            let last = count - 1;
            // Swap with last to preserve LIFO ordering of remaining
            // entries — same shape as the C `arr_pool_blocks_[i] =
            // arr_pool_blocks_[last]`.
            if idx != last {
                BLOCKS[idx].store(BLOCKS[last].load(Ordering::Relaxed), Ordering::Relaxed);
                CAPS[idx].store(CAPS[last].load(Ordering::Relaxed), Ordering::Relaxed);
            }
            BLOCKS[last].store(ptr::null_mut(), Ordering::Relaxed);
            CAPS[last].store(0, Ordering::Relaxed);
            COUNT.store(last, Ordering::Relaxed);
            return p;
        }
        i -= 1;
    }
    ptr::null_mut()
}

/// Try to push a block back into the pool. Returns `true` if accepted,
/// `false` if pool is full (caller falls through to libc free).
#[inline]
pub fn push(p: *mut u8, cap: u64) -> bool {
    let count = COUNT.load(Ordering::Relaxed);
    if count >= POOL_SLOTS {
        return false;
    }
    BLOCKS[count].store(p, Ordering::Relaxed);
    CAPS[count].store(cap, Ordering::Relaxed);
    COUNT.store(count + 1, Ordering::Relaxed);
    true
}

/// Cells of arrays that grew, each still holding the slot buffer it
/// spilled into. An empty literal that is about to be pushed to
/// (`const out: T[] = []` filled in a loop, once per call) otherwise
/// pays a malloc for its cell, a malloc and a realloc for its buffer
/// and two frees for every list it builds; handing it a cell whose
/// buffer already has room makes those pushes stores.
pub const SPILLED_SLOTS: usize = 8;

/// Buffers above this many slots are released, not kept: the stack
/// holds at most `SPILLED_SLOTS * SPILLED_CAP_MAX * 8` bytes (64 KB).
/// A loop that reserves room for its appends up front (one slot per
/// character of a line, and more) lands above a few hundred slots; a
/// lower bound frees such a buffer at every release and the next
/// reserve on the recycled cell reallocates and copies it again. The
/// bound is also the most slack an empty array taken from the stack
/// carries, 8 KB.
pub const SPILLED_CAP_MAX: u64 = 1024;

static SPILLED: [AtomicPtr<u8>; SPILLED_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; SPILLED_SLOTS];

static SPILLED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// The most recently kept spilled cell, or null.
#[inline]
pub fn pop_spilled() -> *mut u8 {
    let count = SPILLED_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return ptr::null_mut();
    }
    SPILLED_COUNT.store(count - 1, Ordering::Relaxed);
    SPILLED[count - 1].load(Ordering::Relaxed)
}

/// Keep a spilled cell; `false` when the stack is full.
#[inline]
pub fn push_spilled(p: *mut u8) -> bool {
    let count = SPILLED_COUNT.load(Ordering::Relaxed);
    if count >= SPILLED_SLOTS {
        return false;
    }
    SPILLED[count].store(p, Ordering::Relaxed);
    SPILLED_COUNT.store(count + 1, Ordering::Relaxed);
    true
}
