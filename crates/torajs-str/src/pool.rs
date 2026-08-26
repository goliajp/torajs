//! Small-Str LIFO pool — thread-local recycler for short-lived
//! `≤ STR_POOL_PAYLOAD` byte strings.
//!
//! The pool stores only the uniform `header(16) + payload(16) = 32
//! byte` block size class. Tight loops like `a + b` (where `+` is
//! string concat) or `s.split(',').forEach(...)` thrash through a
//! few of these blocks per iteration; recycling them via the pool
//! turns malloc/free calls into pointer-pop / pointer-push.
//!
//! ## Single-threaded by contract, `Atomic*` for safety story
//!
//! tora's runtime is single-threaded today (JS spec's single
//! event-loop model). Using `static mut [*mut u8; N]` would
//! compile to the same instructions but trip the Rust 2024
//! `static_mut_refs` lint. `AtomicPtr` + `AtomicUsize` under
//! `Ordering::Relaxed` codegen identically and keep the API
//! `&'static` clean. If threading ever lands, the pool will need a
//! per-thread `RefCell` (or `thread_local!`) variant — but that
//! API change is explicit at that point.
//!
//! ## Bounded slot count
//!
//! 32 slots is large enough to absorb tight loops without bloat
//! (a worst-case `for (let i = 0; i < 32; i++) { acc = acc + 'x'; }`
//! recycles within the pool); once full, additional `push()` calls
//! fall through to `false` so the caller can `libc::free` instead.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::layout::{STR_POOL_PAYLOADS, STR_POOL_SLOTS};

/// One LIFO per payload class ([`STR_POOL_PAYLOADS`]).
const N_CLASSES: usize = STR_POOL_PAYLOADS.len();

/// LIFO slot array. `SLOTS[0..COUNT]` is occupied; the rest is
/// undefined. `pop()` reads `SLOTS[COUNT - 1]` and decrements;
/// `push()` writes `SLOTS[COUNT]` and increments.
static SLOTS: [[AtomicPtr<u8>; STR_POOL_SLOTS]; N_CLASSES] =
    [const { [const { AtomicPtr::new(ptr::null_mut()) }; STR_POOL_SLOTS] }; N_CLASSES];
static COUNT: [AtomicUsize; N_CLASSES] = [const { AtomicUsize::new(0) }; N_CLASSES];

// Every index below is `get`, never `[]`: `pool_class_of` only
// answers a class below `N_CLASSES` and `push` keeps every count
// within `STR_POOL_SLOTS`, but the compiler cannot see either, and a
// bounds check that can never fail still links the whole
// `panic_bounds_check` rendering path — `Display for usize`,
// `Formatter::pad_integral`, the char counter — 5 KB of `core` text
// in a program that prints one integer (r502: `str_alloc_pooled` was
// the empty program's only edge into it). An out-of-range class
// answers "not pooled" instead.

/// Pop the most-recently-pushed block, or `None` if the pool is
/// empty. The popped block's bytes are uninitialized — caller
/// must write the header + len + payload before exposing it.
#[inline]
pub fn pop(class: usize) -> Option<NonNull<u8>> {
    let counter = COUNT.get(class)?;
    let count = counter.load(Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let new_count = count - 1;
    let slot = SLOTS.get(class)?.get(new_count)?;
    counter.store(new_count, Ordering::Relaxed);
    let p = slot.swap(ptr::null_mut(), Ordering::Relaxed);
    // `swap` to null clears the slot so a leaked debug walk
    // doesn't think the pool still owns it. `p` was non-null when
    // we pushed it, so `NonNull::new` is `Some` in non-corrupt
    // builds; using the constructor instead of `unchecked` keeps
    // a panic on accidental corruption (debug-only via
    // `expect_none()` would be wrong since prod hits this every
    // string drop).
    NonNull::new(p)
}

/// Push a freed block onto the LIFO. Returns `true` if accepted,
/// `false` if the pool was full (caller should `libc::free`
/// instead).
///
/// The caller transfers ownership of the block — after a
/// successful push, the block must not be touched until a later
/// `pop()` retrieves it.
#[inline]
pub fn push(class: usize, p: NonNull<u8>) -> bool {
    let (Some(counter), Some(slots)) = (COUNT.get(class), SLOTS.get(class)) else {
        return false;
    };
    let count = counter.load(Ordering::Relaxed);
    let Some(slot) = slots.get(count) else {
        return false;
    };
    slot.store(p.as_ptr(), Ordering::Relaxed);
    counter.store(count + 1, Ordering::Relaxed);
    true
}

/// Current number of blocks held in the pool. Test / bench /
/// debug-instrumentation only; production code should never
/// branch on this — it makes the per-call behavior
/// state-dependent.
#[inline]
pub fn occupancy() -> usize {
    COUNT.iter().map(|c| c.load(Ordering::Relaxed)).sum()
}

/// Reset the pool to empty. Used between unit tests so a test that
/// pushes blocks doesn't leak occupancy into the next test's pop
/// expectations.
///
/// Production callers should never invoke this — leaks any
/// blocks currently held. The function does not free the slots
/// (the pool never owned them in the libc-allocator sense; it
/// holds them on behalf of `__torajs_str_free`).
#[doc(hidden)]
pub fn clear_for_test() {
    for (counter, slots) in COUNT.iter().zip(SLOTS.iter()) {
        for slot in slots.iter() {
            slot.store(ptr::null_mut(), Ordering::Relaxed);
        }
        counter.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Pool is a process-global static; serialize tests so they
    // don't observe each other's pushes.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn fresh_block(addr: usize) -> NonNull<u8> {
        // Just an integer-shaped pointer — these tests never
        // dereference it, only round-trip through the pool.
        NonNull::new(addr as *mut u8).unwrap()
    }

    #[test]
    fn pop_empty_returns_none() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_for_test();
        for class in 0..N_CLASSES {
            assert!(pop(class).is_none(), "class {class}");
        }
    }

    #[test]
    fn push_pop_lifo_order() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_for_test();
        let a = fresh_block(0x1000);
        let b = fresh_block(0x2000);
        let c = fresh_block(0x3000);
        assert!(push(0, a));
        assert!(push(0, b));
        assert!(push(0, c));
        assert_eq!(occupancy(), 3);
        assert_eq!(pop(0).unwrap(), c);
        assert_eq!(pop(0).unwrap(), b);
        assert_eq!(pop(0).unwrap(), a);
        assert!(pop(0).is_none());
    }

    #[test]
    fn push_rejects_when_full() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_for_test();
        for i in 0..STR_POOL_SLOTS {
            assert!(push(0, fresh_block(0x10000 + i)));
        }
        assert_eq!(occupancy(), STR_POOL_SLOTS);
        assert!(!push(0, fresh_block(0xDEAD)));
        // a full class does not close the others; `occupancy` is the
        // sum across classes, so the accepted push shows up there
        assert!(push(1, fresh_block(0xBEEF)));
        assert_eq!(occupancy(), STR_POOL_SLOTS + 1);
        // Pool is now full of fake integer-shaped pointers. They are
        // NOT valid memory — leaving them in the global pool causes
        // the next `StrBlock::alloc` (in this test binary, e.g. the
        // first substr test) to `pop()` one of them and dereference
        // garbage → SIGSEGV. Clear before releasing the lock so the
        // pool's process-global state is fresh for the next test.
        clear_for_test();
    }
}
