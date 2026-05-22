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
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::layout::STR_POOL_SLOTS;

/// LIFO slot array. `SLOTS[0..COUNT]` is occupied; the rest is
/// undefined. `pop()` reads `SLOTS[COUNT - 1]` and decrements;
/// `push()` writes `SLOTS[COUNT]` and increments.
static SLOTS: [AtomicPtr<u8>; STR_POOL_SLOTS] = [const { AtomicPtr::new(ptr::null_mut()) }; 32];
static COUNT: AtomicUsize = AtomicUsize::new(0);

/// Pop the most-recently-pushed block, or `None` if the pool is
/// empty. The popped block's bytes are uninitialized — caller
/// must write the header + len + payload before exposing it.
#[inline]
pub fn pop() -> Option<NonNull<u8>> {
    let count = COUNT.load(Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let new_count = count - 1;
    COUNT.store(new_count, Ordering::Relaxed);
    let p = SLOTS[new_count].swap(ptr::null_mut(), Ordering::Relaxed);
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
pub fn push(p: NonNull<u8>) -> bool {
    let count = COUNT.load(Ordering::Relaxed);
    if count >= STR_POOL_SLOTS {
        return false;
    }
    SLOTS[count].store(p.as_ptr(), Ordering::Relaxed);
    COUNT.store(count + 1, Ordering::Relaxed);
    true
}

/// Current number of blocks held in the pool. Test / bench /
/// debug-instrumentation only; production code should never
/// branch on this — it makes the per-call behavior
/// state-dependent.
#[inline]
pub fn occupancy() -> usize {
    COUNT.load(Ordering::Relaxed)
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
    for slot in SLOTS.iter() {
        slot.store(ptr::null_mut(), Ordering::Relaxed);
    }
    COUNT.store(0, Ordering::Relaxed);
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
        assert!(pop().is_none());
    }

    #[test]
    fn push_pop_lifo_order() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_for_test();
        let a = fresh_block(0x1000);
        let b = fresh_block(0x2000);
        let c = fresh_block(0x3000);
        assert!(push(a));
        assert!(push(b));
        assert!(push(c));
        assert_eq!(occupancy(), 3);
        assert_eq!(pop().unwrap(), c);
        assert_eq!(pop().unwrap(), b);
        assert_eq!(pop().unwrap(), a);
        assert!(pop().is_none());
    }

    #[test]
    fn push_rejects_when_full() {
        let _g = TEST_LOCK.lock().unwrap();
        clear_for_test();
        for i in 0..STR_POOL_SLOTS {
            assert!(push(fresh_block(0x10000 + i)));
        }
        assert_eq!(occupancy(), STR_POOL_SLOTS);
        assert!(!push(fresh_block(0xDEAD)));
        assert_eq!(occupancy(), STR_POOL_SLOTS);
        // Pool is now full of fake integer-shaped pointers. They are
        // NOT valid memory — leaving them in the global pool causes
        // the next `StrBlock::alloc` (in this test binary, e.g. the
        // first substr test) to `pop()` one of them and dereference
        // garbage → SIGSEGV. Clear before releasing the lock so the
        // pool's process-global state is fresh for the next test.
        clear_for_test();
    }
}
