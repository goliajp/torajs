//! Initial-cap dynobj LIFO pool — recycler for the uniform
//! header-cell + initial-cap-store PAIR (32B + 144B; same shape as
//! `torajs-str`'s small-Str pool). Each slot holds a header pointer
//! whose store pointer still carries a live `store_bytes(8)` block —
//! push keeps the pair wired, pop hands both back in one pointer.
//!
//! Round 5 regex re-decomposition attack #3 — the regex wire-back
//! attaches `index` / `input` / `groups` to every match result via a
//! fresh props dynobj, so tight `str.match(re)` loops allocate + free
//! two `calloc`s per iteration (and macOS 26's xzone malloc zeroes on
//! free, making the free as expensive as the alloc). The pool turns
//! that round trip into pointer-pop / pointer-push; the popped pair is
//! re-initialised by the allocator (header + counts + 32-byte index
//! fill ≈ 48 bytes written vs. two callocs).
//!
//! Only never-grown objects (`cap == DYNOBJ_INITIAL_CAP`) are pooled —
//! a grown object's store has a different byte size, so the pair takes
//! the plain two-block free path. A store swapped OUT by resize is a
//! bare block with no header and never enters the pool.
//!
//! Single-threaded by contract, `Atomic*` under `Relaxed` for the
//! same safety story as `torajs-str::pool` (identical codegen, no
//! `static_mut_refs` lint; threading would swap in a `thread_local!`
//! variant explicitly).

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Bounded slot count — enough to absorb one hot loop's transient
/// (typically 1-2 live dynobjs per iteration) without hoarding.
pub const DYNOBJ_POOL_SLOTS: usize = 32;

/// LIFO slot array. `SLOTS[0..COUNT]` is occupied; the rest is
/// undefined.
static SLOTS: [AtomicPtr<u8>; DYNOBJ_POOL_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; DYNOBJ_POOL_SLOTS];
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// Pop the most-recently-pushed header (its store still wired), or
/// `None` if the pool is empty. Both blocks' bytes are stale — the
/// allocator must re-initialise header, counts and hash index before
/// exposing the pair.
#[inline]
pub fn pop() -> Option<NonNull<u8>> {
    let count = COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let new_count = count - 1;
    COUNT.store(new_count, Ordering::Relaxed);
    let p = SLOTS[new_count].swap(ptr::null_mut(), Ordering::Relaxed);
    NonNull::new(p)
}

/// Push a dead initial-cap header (store still wired) onto the LIFO.
/// Returns `true` if accepted, `false` if the pool was full (caller
/// frees both blocks instead). The caller transfers ownership of the
/// pair.
#[inline]
pub fn push(p: NonNull<u8>) -> bool {
    let count = COUNT.load(Ordering::Relaxed);
    if count >= DYNOBJ_POOL_SLOTS {
        return false;
    }
    SLOTS[count].store(p.as_ptr(), Ordering::Relaxed);
    COUNT.store(count + 1, Ordering::Relaxed);
    true
}

/// Current pool occupancy. Test / debug instrumentation only.
#[inline]
pub fn len() -> usize {
    COUNT.load(Ordering::Relaxed)
}
