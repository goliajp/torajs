//! v0.7-A5 16-b — `std::sync::Mutex<T>` drop-in replacement.
//!
//! Backed by macOS `__ulock_wait` / `__ulock_wake` raw syscalls
//! (XNU sysno 515 / 516) and Linux futex (planned). No libc / no
//! `pthread_mutex_*` symbols pulled into the user binary.
//!
//! ## Why
//!
//! Step 16-d (`#[global_allocator]` cutover) dropped the libc
//! malloc family from the user binary's `nm` undef list (49 → 11
//! undefs); the 7 residual `_pthread_mutex_*` symbols come from
//! `std::sync::Mutex` use sites that internally call pthread_mutex
//! on macOS. Replacing those sites with `torajs_mutex::Mutex`
//! drops the 7 pthread_mutex syms and brings the libSystem.dylib
//! drop one step closer (16-b).
//!
//! ## Algorithm
//!
//! Three-state futex mutex (Drepper's pattern, also valid for
//! macOS `__ulock_*`). The state is a `u32` atomic with values:
//!
//! - **0** — unlocked
//! - **1** — locked, no waiters
//! - **2** — locked, one or more waiters parked on `__ulock_wait`
//!
//! ```text
//! lock():
//!     fast: CAS 0 -> 1 (acquire);   success → got it
//!     spin: ~40 spins re-trying the CAS (uncontended contention,
//!           e.g. brief lock-overlap on multi-core)
//!     slow: swap state to 2; if prior was 0, got it (no waiters
//!           queued); otherwise `__ulock_wait(addr=&state, value=2)`
//!           and re-swap on wake
//!
//! unlock():
//!     swap state to 0 (release); if prior was 2 there are waiters
//!     parked, call `__ulock_wake(addr=&state)` to wake one
//! ```
//!
//! ## Why `UL_COMPARE_AND_WAIT` not `UL_UNFAIR_LOCK`
//!
//! `UL_UNFAIR_LOCK = 2` is the XNU-specific os_unfair_lock backing
//! that expects `addr` to hold the owner TID — it would not map
//! cleanly to Linux futex when that backend lands. `UL_COMPARE_AND_WAIT
//! = 1` is the futex-shape primitive that matches Linux futex(2)'s
//! FUTEX_WAIT semantics directly. Cross-platform consistency wins
//! the upper hand vs the marginal os_unfair_lock fairness benefit
//! (which torajs's targeted Mutex use sites — argv state, symbol
//! table, fnprops cache — don't need under their low-contention
//! workloads).
//!
//! ## API parity with `std::sync::Mutex`
//!
//! The public surface mirrors std::sync::Mutex for drop-in
//! replacement at use sites:
//!
//! - `Mutex::new(value)` — **const fn**, matches std's
//!   `Mutex::new` constness required by `static MUTEX: Mutex<T> =
//!   Mutex::new(...)` patterns (e.g. `torajs-process` ARGV_STATE).
//! - `lock(&self) -> MutexGuard<'_, T>` — never returns a
//!   `Result<>` (no poisoning — we abort on panic via
//!   torajs-panic-runtime, so a poisoned mutex is unreachable).
//! - `try_lock(&self) -> Option<MutexGuard<'_, T>>`.
//! - `MutexGuard<'_, T>` impls `Deref` + `DerefMut` + `Drop`.

#![no_std]

use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};

use torajs_syscall::sysno::{SYS_ULOCK_WAIT, SYS_ULOCK_WAKE, UL_COMPARE_AND_WAIT};
use torajs_syscall::{syscall3, syscall6};

const UNLOCKED: u32 = 0;
const LOCKED_NO_WAITERS: u32 = 1;
const LOCKED_HAS_WAITERS: u32 = 2;

/// Number of CPU spin-yields between CAS retries before we cross
/// into the syscall-backed wait path. Picked from typical futex
/// mutex tunings (Rust `parking_lot` uses 40, glibc nptl uses
/// ~100). Lower values trade spin power for higher syscall churn
/// on uncontended micro-contention; higher values burn CPU when
/// the holder is descheduled. 40 matches parking_lot's default.
const SPIN_RETRY_BUDGET: u32 = 40;

/// Replacement for `std::sync::Mutex<T>` — same shape, futex-style
/// backing, no libc dependency.
pub struct Mutex<T: ?Sized> {
    state: AtomicU32,
    data: UnsafeCell<T>,
}

// SAFETY: Mutex provides interior mutability + cross-thread
// synchronization via the atomic state machine + ulock-park
// protocol. T need only be `Send`; Sync is provided by the mutex
// itself.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    /// Create a mutex around `value`. **const fn** for use in
    /// `static MUTEX: Mutex<T> = Mutex::new(...)` patterns.
    pub const fn new(value: T) -> Self {
        Self {
            state: AtomicU32::new(UNLOCKED),
            data: UnsafeCell::new(value),
        }
    }

    /// Unwrap the inner value. Mutex is consumed.
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquire the lock; block until available. Returns a guard
    /// that releases the lock on drop.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // Fast path — uncontended.
        if self
            .state
            .compare_exchange(
                UNLOCKED,
                LOCKED_NO_WAITERS,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return MutexGuard { mutex: self };
        }
        self.lock_contended();
        MutexGuard { mutex: self }
    }

    /// Non-blocking lock attempt. Returns `None` if the mutex is
    /// already held.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        if self
            .state
            .compare_exchange(
                UNLOCKED,
                LOCKED_NO_WAITERS,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            Some(MutexGuard { mutex: self })
        } else {
            None
        }
    }

    /// Mutable access without locking — sound because `&mut self`
    /// proves no other reference exists. Matches `std::sync::Mutex::get_mut`.
    pub fn get_mut(&mut self) -> &mut T {
        // SAFETY: we have &mut self, so no other thread can hold
        // a reference to the inner data.
        unsafe { &mut *self.data.get() }
    }

    /// Cold path: brief spin then ulock-park until unlocked.
    #[cold]
    #[inline(never)]
    fn lock_contended(&self) {
        // Spin a bit — handles tight overlap on multi-core where
        // the holder is about to release.
        for _ in 0..SPIN_RETRY_BUDGET {
            if self.state.load(Ordering::Relaxed) == UNLOCKED
                && self
                    .state
                    .compare_exchange(
                        UNLOCKED,
                        LOCKED_NO_WAITERS,
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                return;
            }
            core::hint::spin_loop();
        }

        // Park / re-try loop. Setting state to 2 (HAS_WAITERS)
        // before the syscall is what tells the unlock path to
        // call `__ulock_wake`.
        loop {
            // Swap-to-2; if prior was 0 we got the lock without
            // ever needing to park (lock holder just released
            // between our spin and our swap).
            let prev = self.state.swap(LOCKED_HAS_WAITERS, Ordering::Acquire);
            if prev == UNLOCKED {
                return;
            }
            // prev was 1 or 2 — somebody holds it; park until
            // unlock wakes us. Timeout 0 = wait forever. Re-check
            // state on wake — spurious wakes possible.
            unsafe {
                ulock_wait(
                    UL_COMPARE_AND_WAIT,
                    &self.state as *const AtomicU32 as *const c_void,
                    LOCKED_HAS_WAITERS as u64,
                    0,
                );
            }
        }
    }

    /// Release the lock. Called by `MutexGuard::drop`. Wakes one
    /// parked waiter if any.
    fn unlock(&self) {
        if self.state.swap(UNLOCKED, Ordering::Release) == LOCKED_HAS_WAITERS {
            // We had waiters parked — wake exactly one.
            unsafe {
                ulock_wake(
                    UL_COMPARE_AND_WAIT,
                    &self.state as *const AtomicU32 as *const c_void,
                    0,
                );
            }
        }
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// RAII guard releasing the lock on drop.
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    mutex: &'a Mutex<T>,
}

// SAFETY: the guard ties the inner reference's lifetime to the
// scope where the mutex is locked; T: Send/Sync transitivity
// matches std::sync::MutexGuard.
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: holding the guard proves the mutex is locked
        // and no other reference exists.
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: same as Deref but exclusive.
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

/// Raw `__ulock_wait` syscall. Park the current thread until
/// `*(u32 *)addr != value` or `timeout_us` elapses (0 = forever).
/// Returns the syscall's `x0` return verbatim (≥ 0 success, < 0
/// = -errno).
///
/// # Safety
///
/// `addr` must point to a valid `AtomicU32` aligned to 4 bytes.
/// The atomic value at `addr` must remain valid for the duration
/// of the park.
#[inline]
unsafe fn ulock_wait(operation: u32, addr: *const c_void, value: u64, timeout_us: u32) -> i64 {
    // __ulock_wait(uint32_t op, void *addr, uint64_t value, uint32_t timeout_us)
    // 4-arg syscall — use the 6-arg trampoline with explicit zeros
    // for the unused tail so `timeout_us` lands in x3 and not x2.
    unsafe {
        syscall6(
            SYS_ULOCK_WAIT,
            operation as i64,
            addr as i64,
            value as i64,
            timeout_us as i64,
            0,
            0,
        )
    }
}

/// Raw `__ulock_wake` syscall. Wake threads parked on the same
/// addr. Returns wake count (≥ 0 success, < 0 = -errno).
///
/// # Safety
///
/// `addr` is the same as the one passed to `ulock_wait`.
#[inline]
unsafe fn ulock_wake(operation: u32, addr: *const c_void, wake_value: u64) -> i64 {
    // __ulock_wake(uint32_t op, void *addr, uint64_t wake_value)
    unsafe {
        syscall3(
            SYS_ULOCK_WAKE,
            operation as i64,
            addr as i64,
            wake_value as i64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uncontended fast path: lock + unlock round-trip.
    #[test]
    fn lock_unlock_uncontended() {
        let m = Mutex::new(0u64);
        {
            let mut g = m.lock();
            *g = 42;
        }
        let g = m.lock();
        assert_eq!(*g, 42);
    }

    /// `try_lock` returns Some on uncontended, None on contended.
    #[test]
    fn try_lock_behavior() {
        let m = Mutex::new(0i32);
        let g1 = m.try_lock();
        assert!(g1.is_some(), "try_lock on free mutex should succeed");
        // Second try_lock from the same thread (lock already held
        // by g1) must fail — std::sync::Mutex would deadlock if
        // re-entered, this returns None.
        let g2 = m.try_lock();
        assert!(g2.is_none(), "try_lock on held mutex should return None");
        drop(g1);
        let g3 = m.try_lock();
        assert!(g3.is_some(), "try_lock after drop should succeed");
    }

    /// const-new works for static items — confirms ARGV_STATE-shape
    /// `static M: Mutex<T> = Mutex::new(...)` pattern.
    #[test]
    fn const_new_static() {
        static M: Mutex<u64> = Mutex::new(123);
        let g = M.lock();
        assert_eq!(*g, 123);
    }

    /// Default impl.
    #[test]
    fn default_unlocked_default_value() {
        let m: Mutex<u32> = Mutex::default();
        let g = m.lock();
        assert_eq!(*g, 0);
    }

    /// `get_mut` does not lock.
    #[test]
    fn get_mut_does_not_lock() {
        let mut m = Mutex::new(0u8);
        *m.get_mut() = 7;
        let g = m.lock();
        assert_eq!(*g, 7);
    }

    /// `into_inner` unwraps.
    #[test]
    fn into_inner_unwraps() {
        let m = Mutex::new(99u16);
        assert_eq!(m.into_inner(), 99);
    }

    /// Single-thread fast-path stress — 100k lock/unlock round-trips
    /// in the uncontended CAS path. Catches any UB in the fast path
    /// without exercising the spin / ulock_wait code.
    #[test]
    fn fast_path_stress_single_thread() {
        let m = Mutex::new(0u64);
        for _ in 0..100_000 {
            let mut g = m.lock();
            *g += 1;
        }
        assert_eq!(*m.lock(), 100_000);
    }

    /// Two-thread without Mutex — checks std::thread + Arc work
    /// at all in this test harness. Isolates the SIGSEGV: if this
    /// passes, Mutex's contended path is the culprit (not the
    /// thread spawn surface).
    #[test]
    fn two_threads_atomic_only_no_mutex() {
        extern crate std;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::thread;
        let counter = Arc::new(AtomicU64::new(0));
        let c1 = counter.clone();
        let h1 = thread::spawn(move || {
            for _ in 0..1_000 {
                c1.fetch_add(1, Ordering::Relaxed);
            }
        });
        let c2 = counter.clone();
        let h2 = thread::spawn(move || {
            for _ in 0..1_000 {
                c2.fetch_add(1, Ordering::Relaxed);
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 2_000);
    }

    /// Single-thread re-entrancy via ulock_wake path (state stuck
    /// at 2). Checks that ulock_wake on an addr with no parked
    /// waiters returns OK and doesn't crash.
    #[test]
    fn single_thread_forced_contended_path() {
        let m = Mutex::new(0u64);
        // Force state to LOCKED_HAS_WAITERS without anyone parked
        // (simulates the "spin lost the race" condition).
        m.state.store(LOCKED_HAS_WAITERS, Ordering::Relaxed);
        // Now unlock: prior was 2 → ulock_wake called with 0
        // waiters. Should not crash; wake count returned is 0.
        m.unlock();
        assert_eq!(m.state.load(Ordering::Relaxed), UNLOCKED);
    }

    /// Direct ulock_wait probe — calls the syscall with a quick
    /// timeout (1 ms) on an address whose value MATCHES `value`,
    /// so kernel parks us briefly then returns -ETIMEDOUT.
    /// Verifies the syscall itself is reachable and doesn't
    /// SIGSEGV the caller, and the timeout arg lands correctly.
    #[test]
    fn ulock_wait_with_brief_timeout_does_not_crash() {
        let state = AtomicU32::new(7);
        // state value is 7, we wait while *addr == 7 → kernel parks.
        // Timeout 1000 us = 1 ms; no one wakes us → kernel returns
        // -ETIMEDOUT or similar negative errno.
        let rc = unsafe {
            ulock_wait(
                UL_COMPARE_AND_WAIT,
                &state as *const AtomicU32 as *const c_void,
                7,     // matching value
                1_000, // 1 ms timeout
            )
        };
        // Either rc < 0 (timeout / interrupted) or rc >= 0 (state
        // changed via spurious wake) — both fine. The point is we
        // returned at all without SIGSEGV.
        let _ = rc;
    }

    /// Contended path: two threads racing N increments each.
    /// Final value must be exactly 2N. Exercises the spin +
    /// ulock_wait + ulock_wake protocol.
    #[test]
    fn contended_two_thread_increment() {
        extern crate std;
        use std::sync::Arc;
        use std::thread;
        let n = 10_000u64;
        let m = Arc::new(Mutex::new(0u64));
        let m1 = m.clone();
        let h1 = thread::spawn(move || {
            for _ in 0..n {
                let mut g = m1.lock();
                *g += 1;
            }
        });
        let m2 = m.clone();
        let h2 = thread::spawn(move || {
            for _ in 0..n {
                let mut g = m2.lock();
                *g += 1;
            }
        });
        h1.join().unwrap();
        h2.join().unwrap();
        assert_eq!(*m.lock(), n * 2);
    }

    /// Minimal contended: 1 iter each thread. If this passes but
    /// the 10k version crashes, the bug is volume-induced (e.g.
    /// state corruption over many ulock cycles).
    #[test]
    fn contended_two_thread_one_iter() {
        extern crate std;
        use std::sync::Arc;
        use std::thread;
        let m = Arc::new(Mutex::new(0u64));
        let m1 = m.clone();
        let h1 = thread::spawn(move || {
            let mut g = m1.lock();
            *g += 1;
        });
        let m2 = m.clone();
        let h2 = thread::spawn(move || {
            let mut g = m2.lock();
            *g += 1;
        });
        h1.join().unwrap();
        h2.join().unwrap();
        assert_eq!(*m.lock(), 2);
    }
}
