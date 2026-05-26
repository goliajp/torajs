//! Array allocation + pool-aware free.
//!
//! Port of `runtime_str.c::__torajs_arr_alloc_pooled` + `__torajs_arr_free`
//! (P4.1-b, 2026-05-23).
//!
//! - [`__torajs_arr_alloc_pooled`] — alloc a regular `Array<T>` block.
//!   `cap ≤ POOL_CAP_MAX` first searches [`crate::pool`] for a matching-
//!   cap recycled block; cap match pops + reuses; miss falls through
//!   to libc malloc.
//! - [`__torajs_arr_free`] — drop path's free entry. STATIC_LITERAL =
//!   no-op; SPLIT_BLOCK route to torajs-str's split pool first; small
//!   non-Any cap → arr pool; otherwise libc free. ARR_ANY blocks
//!   (16-byte slots) bypass the pool — the pool's stride assumption
//!   doesn't match.
//!
//! Header init shape (24 bytes):
//! ```text
//! [refcount:4 = 1] [type_tag:2 = TAG_ARR] [flags:2 = 0]
//! [len:8 = 0] [cap:4 = <arg>] [head_offset:4 = 0]
//! ```

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, FLAG_SPLIT_BLOCK, FLAG_STATIC_LITERAL, HeapHeader};

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF, TAG_ARR};
use crate::pool::{POOL_CAP_MAX, POOL_SLOTS, pop_cap_match, push};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);

    /// Cross-tier — torajs-str's split-block pool. Returns 1 if the
    /// block was accepted into the split pool (caller does NOT free),
    /// 0 if the pool was full (caller falls through to libc free).
    /// SPLIT_BLOCK flag marks arrays produced by `String.split` —
    /// their inline Substr layout differs from a regular Array<T>.
    fn __torajs_split_block_free_push(p: *mut u8) -> i32;
}

/// Array cap slot offset (mirrors C macro `__TORAJS_ARR_HDR_CAP_OFF`).
/// Same byte offset as [`layout::ARR_CAP_OFF`] but kept here as a `u8`-
/// indexed read since cap was shrunk to u32 in T-13.5 (high 32 bits =
/// head_offset).
const ARR_CAP_LOW32_OFF: usize = 16;

/// Block size for cap-N regular `Array<T>`: 24-byte header + 8 bytes
/// per slot.
#[inline]
fn block_size_regular(cap: u64) -> usize {
    ARR_SLOTS_OFF + (cap as usize) * 8
}

/// Pool-aware alloc for a regular `Array<T>` (not `Array<Any>`).
/// Returns a fresh `+1`-rc heap pointer.
///
/// # Safety
/// Returned pointer is `cap * 8`-byte slot-sized + 24-byte header.
/// Caller owns; release via `__torajs_arr_drop` (or `__torajs_arr_free`
/// directly if the rc was never incremented).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_pooled(cap: u64) -> *mut u8 {
    let p: *mut u8 = if cap <= POOL_CAP_MAX {
        let recycled = pop_cap_match(cap);
        if !recycled.is_null() {
            recycled
        } else {
            unsafe { malloc(block_size_regular(cap)) as *mut u8 }
        }
    } else {
        unsafe { malloc(block_size_regular(cap)) as *mut u8 }
    };
    unsafe {
        // Header init: rc=1, tag=ARR, flags=0.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = 0;
        // len = 0
        *(p.add(ARR_LEN_OFF) as *mut u64) = 0;
        // cap (u32) + head_offset (u32, T-13.5)
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap as u32;
        *(p.add(ARR_CAP_LOW32_OFF + 4) as *mut u32) = 0;
    }
    p
}

/// `__torajs_arr_alloc(cap)` — top-level Array alloc entry.
///
/// Body used to be an inkwell IR builder (`define_arr_alloc`) that
/// tail-called `arr_alloc_pooled`; collapsed at LTO. Now a direct
/// Rust wrapper preserves the same shape — single delegate call,
/// `#[inline]` to encourage the linker to fold it into the caller.
///
/// # Safety
/// Same contract as [`__torajs_arr_alloc_pooled`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc(cap: u64) -> *mut u8 {
    unsafe { __torajs_arr_alloc_pooled(cap) }
}

/// Pool-aware free. Called by [`crate::drop::__torajs_arr_drop`] on
/// the last-owner path.
///
/// # Safety
/// `p` is either NULL or a valid `Array<T>` / `Array<Any>` heap pointer.
/// SPLIT_BLOCK + STATIC_LITERAL flags are honored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_free(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    // STATIC_LITERAL — `.rodata` blocks never get freed.
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return;
    }
    // SPLIT_BLOCK takes priority — cross-tier to torajs-str's split pool.
    if header.flags & FLAG_SPLIT_BLOCK != 0 {
        if unsafe { __torajs_split_block_free_push(p as *mut u8) } != 0 {
            return;
        }
        // Pool full → fall through to libc free.
    } else {
        // Read cap (low 32 bits at offset 16). Pool only accepts
        // regular Array<T> — Array<Any>'s 16-byte slots wouldn't
        // match the pool's 8-byte stride assumption.
        let cap = unsafe { *((p as *const u8).add(ARR_CAP_LOW32_OFF) as *const u32) } as u64;
        let count = crate::pool::current_count();
        if cap <= POOL_CAP_MAX
            && count < POOL_SLOTS
            && (header.flags & FLAG_ARR_ANY) == 0
            && push(p as *mut u8, cap)
        {
            return;
        }
    }
    unsafe { free(p) };
}
