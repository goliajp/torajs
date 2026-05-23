//! Split-block LIFO pool — per-cap cache for the single-block
//! Arr-with-inline-substrs layout produced by [`crate::split::ops::
//! __torajs_str_split`].
//!
//! Block size depends on cap (24 header + 8*cap slots + 32*cap
//! inline substrs), so the pool keys on `cap` and only recycles
//! blocks whose cap matches the caller's request.
//!
//! 16 slots is enough to absorb tight loops over a single cap
//! value (the dominant pattern: `s.split(',')` on uniform-shape
//! inputs). Multi-cap workloads bypass the pool after the slots
//! fill — the leverage is in single-cap tight loops, not in
//! workloads that produce 17 different cap values.

use core::ptr::{self, NonNull};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::substr::SUBSTR_SIZE;

// ============================================================
// Layout constants (cross-layer — Arr is P4.x territory).
//
// Mirrors `runtime_str.c`:
//   #define __TORAJS_ARR_HDR_SIZE   24
//   #define __TORAJS_ARR_LEN(p)     u64 at +8
//   #define __TORAJS_ARR_CAP(p)     u32 at +16
//   #define __TORAJS_ARR_HEAD(p)    u32 at +20
//
// When `torajs-arr` lands (P4.x torajs-arr crate) these consts
// move there and this file imports them. Until then, defined
// locally so split doesn't reach into Layer-3 unported code.
// ============================================================

pub(crate) const ARR_HDR_SIZE: usize = 24;
pub(crate) const ARR_CAP_OFF: usize = 16;

const POOL_SLOTS: usize = 16;

// ============================================================
// State — slot[i] holds (block ptr, cap). count is the index of
// the next free slot.
// ============================================================

static BLOCKS: [AtomicPtr<u8>; POOL_SLOTS] =
    [const { AtomicPtr::new(ptr::null_mut()) }; POOL_SLOTS];
static CAPS: [AtomicUsize; POOL_SLOTS] = [const { AtomicUsize::new(0) }; POOL_SLOTS];
static COUNT: AtomicUsize = AtomicUsize::new(0);

// ============================================================
// Block sizing
// ============================================================

/// Exact byte size of a split block for the given `cap`:
/// `ARR_HDR_SIZE + cap * 8 + cap * SUBSTR_SIZE` = `24 + 40 * cap`.
#[inline]
pub fn block_size(cap: u64) -> usize {
    let cap_u = cap as usize;
    ARR_HDR_SIZE + cap_u * 8 + cap_u * (SUBSTR_SIZE as usize)
}

// ============================================================
// Pool API
// ============================================================

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
}

/// Pop the most-recently-pushed block whose cap matches
/// `out_count`, or allocate a fresh one if no match. Returns a
/// raw (uninitialized) block — caller writes the Arr header + len
/// + cap + head + N ptr slots + N inline substr structs before
/// exposing it.
///
/// The cap-match scan walks from the most-recent slot down — tight
/// loops on a single cap value hit on the first compare.
#[inline]
pub fn alloc(out_count: u64) -> NonNull<u8> {
    let want = out_count as usize;
    let count = COUNT.load(Ordering::Relaxed);
    if count > 0 {
        // Scan from most-recent slot. Cap stored as usize for
        // direct compare; ptr swapped to null to clear ownership.
        for i in (0..count).rev() {
            if CAPS[i].load(Ordering::Relaxed) == want {
                let p = BLOCKS[i].swap(ptr::null_mut(), Ordering::Relaxed);
                // Swap-remove: move last slot's contents to this
                // slot, then shrink count. Preserves LIFO order for
                // the still-occupied slots.
                let last = count - 1;
                if i != last {
                    let last_p = BLOCKS[last].swap(ptr::null_mut(), Ordering::Relaxed);
                    let last_cap = CAPS[last].load(Ordering::Relaxed);
                    BLOCKS[i].store(last_p, Ordering::Relaxed);
                    CAPS[i].store(last_cap, Ordering::Relaxed);
                }
                COUNT.store(last, Ordering::Relaxed);
                if let Some(nn) = NonNull::new(p) {
                    return nn;
                }
                break; // null entry — fall through to malloc
            }
        }
    }
    // Pool miss — fresh allocation.
    let raw = unsafe { malloc(block_size(out_count)) } as *mut u8;
    NonNull::new(raw).expect("OOM in split block alloc")
}

/// Push a freed split block onto the LIFO. Returns `true` if
/// accepted, `false` if the pool was full (caller should
/// `libc::free` the block instead).
///
/// `cap` is read from the Arr header by the dispatching
/// `__torajs_arr_free`; we trust it as opaque scalar (no header
/// reread here).
#[inline]
pub fn push(p: NonNull<u8>, cap: u64) -> bool {
    let count = COUNT.load(Ordering::Relaxed);
    if count >= POOL_SLOTS {
        return false;
    }
    BLOCKS[count].store(p.as_ptr(), Ordering::Relaxed);
    CAPS[count].store(cap as usize, Ordering::Relaxed);
    COUNT.store(count + 1, Ordering::Relaxed);
    true
}

// ============================================================
// extern "C" wrapper — called from C-side __torajs_arr_free for
// the SPLIT_BLOCK dispatch path.
// ============================================================

/// `__torajs_arr_free` SPLIT_BLOCK dispatch — reads cap (u32 at
/// `ARR_CAP_OFF`) from the block header and pushes onto the
/// split pool. Returns `1` if accepted, `0` if pool full (caller
/// falls through to `libc::free`).
///
/// # Safety
///
/// `p` must point at a valid SPLIT_BLOCK-flagged Arr block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_split_block_free_push(p: *mut u8) -> i32 {
    if p.is_null() {
        return 1; // null is a no-op success — match C `if (p == NULL) return;`
    }
    // SAFETY: caller's contract: p is a SPLIT_BLOCK Arr; cap u32
    // lives at ARR_CAP_OFF (= +16). We read as u32 to mirror C
    // `*(uint32_t *)((uint8_t *)p + 16)`.
    let cap = unsafe { (p.add(ARR_CAP_OFF) as *const u32).read() } as u64;
    // SAFETY: just null-checked.
    let nn = unsafe { NonNull::new_unchecked(p) };
    if push(nn, cap) { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `free` for cleanup. Same extern decl as alloc.rs uses for the
    // Str pool — libc symbol, no crate dep.
    unsafe extern "C" {
        fn free(ptr: *mut c_void);
    }

    fn drain_pool() {
        // The pool is process-global; cargo test runs `#[test]` fns
        // in parallel, so we wrap ALL pool ops in one sequential
        // `#[test]` (this fn) and pre-drain at entry to be safe even
        // against a future test that leaks state.
        while COUNT.load(Ordering::Relaxed) > 0 {
            let count = COUNT.load(Ordering::Relaxed);
            let last = count - 1;
            BLOCKS[last].store(ptr::null_mut(), Ordering::Relaxed);
            CAPS[last].store(0, Ordering::Relaxed);
            COUNT.store(last, Ordering::Relaxed);
        }
    }

    #[test]
    fn block_size_matches_c_formula() {
        assert_eq!(block_size(0), 24);
        assert_eq!(block_size(1), 24 + 8 + 32);
        assert_eq!(block_size(3), 24 + 24 + 96);
        assert_eq!(block_size(10), 24 + 80 + 320);
    }

    #[test]
    fn pool_ffi_full_roundtrip() {
        // Single test exercising all pool ops sequentially to avoid
        // races on the process-global slot array.
        drain_pool();

        // 1. push then pop same cap → same pointer.
        let raw4 = unsafe { malloc(block_size(4)) } as *mut u8;
        let nn4 = NonNull::new(raw4).unwrap();
        assert!(push(nn4, 4));
        assert_eq!(alloc(4).as_ptr(), raw4);

        // 2. push 4, alloc(5) misses → fresh, alloc(4) still hits.
        assert!(push(nn4, 4));
        let popped5 = alloc(5);
        assert_ne!(popped5.as_ptr(), raw4);
        unsafe { free(popped5.as_ptr() as *mut c_void) };
        assert_eq!(alloc(4).as_ptr(), raw4);

        // 3. fill pool to capacity → next push returns false.
        let mut filled = Vec::new();
        for _ in 0..POOL_SLOTS {
            let raw = unsafe { malloc(block_size(2)) } as *mut u8;
            filled.push(raw);
            assert!(push(NonNull::new(raw).unwrap(), 2));
        }
        let extra = unsafe { malloc(block_size(2)) } as *mut u8;
        assert!(!push(NonNull::new(extra).unwrap(), 2));
        // Drain back.
        for _ in 0..POOL_SLOTS {
            let p = alloc(2);
            unsafe { free(p.as_ptr() as *mut c_void) };
        }
        unsafe { free(extra as *mut c_void) };

        // 4. FFI free_push reads cap from ARR_CAP_OFF.
        let raw3 = unsafe { malloc(block_size(3)) } as *mut u8;
        unsafe { (raw3.add(ARR_CAP_OFF) as *mut u32).write(3) };
        assert_eq!(unsafe { __torajs_split_block_free_push(raw3) }, 1);
        assert_eq!(alloc(3).as_ptr(), raw3);
        unsafe { free(raw3 as *mut c_void) };

        // 5. FFI free_push null is no-op success.
        let count_before = COUNT.load(Ordering::Relaxed);
        assert_eq!(
            unsafe { __torajs_split_block_free_push(ptr::null_mut()) },
            1
        );
        assert_eq!(COUNT.load(Ordering::Relaxed), count_before);

        // 6. Clean original cap-4 block (still owned by the test).
        unsafe { free(raw4 as *mut c_void) };
    }
}
