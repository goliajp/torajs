//! Array growth + length-mutation helpers.
//!
//! This module gathers ops that change an array's `len` field — push /
//! reserve / shift (T-13.5 deque) + the spec-validator for `arr.length =
//! N` assignment. Sub-step matrix (P4.1):
//!
//! | Sub  | Adds                                                          |
//! |------|---------------------------------------------------------------|
//! | P4.1-j | `__torajs_arr_set_length_validate` (ES §9.4.2.4 guard)      |
//! | P4.1-k | `__torajs_arr_reserve` (realloc-if-cap-too-small)           |
//! | P4.1-l | `__torajs_arr_push` (typed push with auto-grow)             |
//! | P4.1-m | `__torajs_arr_shift` (T-13.5 deque head_offset fold)        |

use core::ffi::c_void;

/// Offset of the `len` u64 within an array heap block.
pub const ARR_HDR_LEN_OFF: usize = 8;

/// Offset of the `cap` u32 within an array heap block. T-13.5 packed
/// cap (u32) + head_offset (u32) into the 8-byte slot at offset 16
/// (formerly cap was a u64). Mirrors ssa_lower's deque-layout table.
pub const ARR_HDR_CAP_OFF: usize = 16;

/// Offset of the `head_offset` u32 within an array heap block (T-13.5
/// deque packed cap + head).
pub const ARR_HDR_HEAD_OFF: usize = 20;

use crate::layout::{ARR_DATA_PTR_OFF, arr_data, arr_data_is_inline};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_realloc"]
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;

    /// torajs-mmalloc libc-compat malloc — first grow spills the
    /// inline slots region to an independent buffer (B1).
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
}

/// The capacity one growth step moves to: double, and never under 16
/// slots. Doubling from 4 moved a pushed-to array's buffer at 4, 8, 16
/// and 32 elements, four allocate-copy-release rounds for a list of a
/// few dozen where two do; the price is 128 bytes instead of 32 for
/// the buffer of an array pushed to once. Past 16 the step is the
/// doubling it was, so a large array copies no more than before.
#[inline]
pub(crate) fn grown_cap(cap: u64) -> u64 {
    (cap * 2).max(16)
}

/// RFC 20260706-arr-grow-alias-stability B1 — grow the data buffer to
/// `new_cap` slots (8-byte stride; typed and Array<Any> share it).
/// The CELL never moves, so every alias — caller bindings, container
/// slots, struct fields, globals — stays valid across grow. First
/// grow spills the inline region to a malloc'd buffer (full physical
/// range copied, preserving deque head layout); later grows realloc
/// the buffer in place.
pub(crate) unsafe fn grow_data_buffer(arr: *mut u8, new_cap: u64) {
    // the header keeps the capacity as a u32
    debug_assert!(new_cap <= u32::MAX as u64);
    unsafe {
        let new_bytes = (new_cap as usize) * 8;
        let old_data = arr_data(arr);
        let new_data = if arr_data_is_inline(arr) {
            let buf = malloc(new_bytes) as *mut u8;
            let old_cap = *(arr.add(ARR_HDR_CAP_OFF) as *const u32) as usize;
            core::ptr::copy_nonoverlapping(old_data, buf, old_cap * 8);
            // a split block's size follows from its own cap, which the
            // header stops describing now; the slot table just copied
            // out is dead, so its first word keeps it for the release
            if (*(arr as *const torajs_rc::HeapHeader)).flags & torajs_rc::FLAG_SPLIT_BLOCK != 0 {
                *(old_data as *mut u64) = old_cap as u64;
            }
            buf
        } else {
            realloc(old_data as *mut c_void, new_bytes) as *mut u8
        };
        *(arr.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = new_data;
        *(arr.add(ARR_HDR_CAP_OFF) as *mut u32) = new_cap as u32;
    }
}

/// Shared §10.4.2.5 length-grow tail — compact the deque head, grow
/// the buffer, fill `[old, new_n)` with `fill`, write `len`, and mark
/// every new index a HOLE (RFC 20260721 刀 5 G3: a grown slot is NOT
/// an own property — `1 in x` after `x.length = 2` is false and
/// element reads continue to the prototype digit keys).
pub(crate) unsafe fn grow_slots_as_holes(arr: *mut u8, old: u64, new_n: u64, fill: u64) {
    unsafe {
        let head = *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) as usize;
        if head > 0 {
            let raw = arr_data(arr);
            core::ptr::copy(raw.add(head * 8), raw, old as usize * 8);
            *(arr.add(ARR_HDR_HEAD_OFF) as *mut u32) = 0;
        }
        let cap = *(arr.add(ARR_HDR_CAP_OFF) as *const u32) as u64;
        if cap < new_n {
            grow_data_buffer(arr, new_n);
        }
        let slots = arr_data(arr);
        for i in old..new_n {
            *(slots.add(i as usize * 8) as *mut u64) = fill;
        }
        *(arr.add(ARR_HDR_LEN_OFF) as *mut u64) = new_n;
        crate::define_hole::mark_hole_range(arr as *mut c_void, old, new_n);
    }
}

/// Push `val` onto the end of `arr`, growing the data buffer if
/// needed. Returns `arr` unchanged — the cell never moves (B1);
/// callers that store the return value back are harmless no-ops
/// (write-back chain retires in B3).
///
/// ```text
/// fast path: head + len < cap → store immediately
/// need-room:
///   if head > 0: memmove(data, data + head*8, len*8); head = 0  // T-13.5 compact
///   if len * 2 >= cap: grow_data_buffer(grown_cap(cap))
/// store: *(arr_data(arr) + (head + len)*8) = val; len += 1
/// ```
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live Array<T> heap block (8-byte
/// slot stride — Array<Any> has its own push_any path in
/// [`crate::any`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8 {
    // A new index on a non-extensible receiver is a TypeError
    // (`__torajs_arr_extend_guard`). Nothing to release here, and that
    // is deliberate: on this lane the raw slot value is BORROWED at
    // call time — the lowering incs the array's stake only after the
    // call returns — so a kernel-side drop would release a stake the
    // array never received. The lowering's throw check right after
    // the call (`emit_grow_kernel_checked`) is what settles the temp.
    if unsafe { crate::define_length::__torajs_arr_extend_guard(arr as *const core::ffi::c_void) }
        != 0
    {
        return arr;
    }
    let len = unsafe { *(arr.add(ARR_HDR_LEN_OFF) as *const u64) } as i64;
    let cap = unsafe { *(arr.add(ARR_HDR_CAP_OFF) as *const u32) } as i64;
    let head = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as i64;
    let phys_used = head + len;

    if phys_used >= cap {
        if head > 0 {
            unsafe {
                let raw_data = arr_data(arr);
                let src = raw_data.add((head as usize) * 8);
                core::ptr::copy(src, raw_data, (len as usize) * 8);
                *(arr.add(ARR_HDR_HEAD_OFF) as *mut u32) = 0;
            }
        }
        // A fold that would leave the buffer half full or more grows
        // it as well: the next fold is then more than `len` pushes
        // away, so a queue moves at most one slot per push. A fold into
        // a buffer just over half full came back after a handful of
        // pushes and moved the whole queue each time. With no head
        // (len == cap here, an empty cap-0 block included) it grows,
        // as before.
        if len * 2 >= cap {
            unsafe { grow_data_buffer(arr, grown_cap(cap as u64)) };
        }
    }

    // Re-load head — compact path may have reset it to 0.
    let head_now = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as i64;
    unsafe {
        let slot = arr_data(arr).add(((head_now + len) as usize) * 8) as *mut i64;
        *slot = val;
        *(arr.add(ARR_HDR_LEN_OFF) as *mut u64) = (len + 1) as u64;
    }
    arr
}

/// [`__torajs_arr_push`] under a second name: the out-of-line arm of
/// a push the lowering opened in place. Codegen opens every
/// `__torajs_arr_push` call again; a call to this one stays a call.
///
/// # Safety
/// As [`__torajs_arr_push`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_cold(arr: *mut u8, val: i64) -> *mut u8 {
    unsafe { __torajs_arr_push(arr, val) }
}

/// T-13.5 O(1) deque shift: pop and return `arr[0]`.
///
/// Algorithm (1:1 port of ssa_inkwell::define_arr_shift, ~70 LOC IR @
/// L2841-2920; was originally alwaysinline IR specifically so LLVM
/// inlined the body into the caller's fifo-queue hot loop):
///
/// ```text
/// head  = *(u32*)(arr + 20)
/// v     = *(i64*)(arr + 24 + head*8)   // logical[0]
/// *(u32*)(arr + 20) = head + 1         // bump head_offset
/// *(u64*)(arr +  8) -= 1               // dec len
/// return v
/// ```
///
/// **Perf note**: porting from inkwell IR to Rust extern "C" gives up
/// the `alwaysinline` cross-tier inlining (Rust's `#[inline(always)]`
/// conflicts with `#[unsafe(no_mangle)]`). The fifo-queue benchmark
/// will now show a `bl __torajs_arr_shift` cross-tier call where the
/// IR version inlined the 4 memory ops. Cross-tier LTO across
/// libtorajs_arr.a should still inline when fat-LTO is enabled at
/// `tr build` time; thin-LTO will leave the call.
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a non-empty Array<T> heap block
/// (8-byte slot stride). Caller's SSA-level shift dispatch guarantees
/// len > 0 (bug-327 C1: the `emit_pop_shift_empty_guard` CondBr —
/// the empty path never reaches this helper).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_shift(arr: *mut u8) -> i64 {
    unsafe {
        debug_assert!(
            *(arr.add(ARR_HDR_LEN_OFF) as *const u64) > 0,
            "__torajs_arr_shift on empty array — SSA empty guard missing"
        );
        // A sealed receiver never reaches the head bump: the lowering's
        // lock guard (`__torajs_arr_shift_guard`) refuses it first, and
        // the any-lane kernel checks before delegating here.
        debug_assert!(
            crate::define::header_flags(arr as *const c_void) & torajs_rc::FLAG_SEALED == 0,
            "__torajs_arr_shift on a sealed array — lock guard missing"
        );
        let head_p = arr.add(ARR_HDR_HEAD_OFF) as *mut u32;
        let head = *head_p as usize;
        let slot = arr_data(arr).add(head * 8) as *const i64;
        let v = *slot;
        *head_p = (head + 1) as u32;
        let len_p = arr.add(ARR_HDR_LEN_OFF) as *mut u64;
        *len_p -= 1;
        if crate::define::header_flags(arr as *const c_void) & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0
        {
            shift_holes(arr, *len_p + 1);
        }
        v
    }
}

/// §23.1.3.27 step 6 moves every index down one, holes included — a
/// typed hole is a shadow mark keyed by index, so the marks move with
/// the slots the head bump just moved. Out of line: the queue loop
/// never carries a mark.
#[cold]
#[inline(never)]
unsafe fn shift_holes(arr: *mut u8, old_len: u64) {
    unsafe {
        crate::define_hole::move_holes(arr.cast(), old_len, old_len - 1, |i| i.checked_sub(1))
    };
}

/// Grow an array's data buffer to fit at least `new_cap` elements.
/// Cap-equal short-circuits to no-op. Returns `arr` unchanged — the
/// cell never moves (B1); the historical "caller must adopt the
/// return value" contract is now a harmless no-op.
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live array heap block (non-NULL,
/// allocated via `__torajs_arr_alloc*`); `new_cap` non-negative.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_reserve(arr: *mut u8, new_cap: i64) -> *mut u8 {
    let cap = unsafe { *(arr.add(ARR_HDR_CAP_OFF) as *const u32) } as i64;
    if cap >= new_cap {
        return arr;
    }
    unsafe { grow_data_buffer(arr, new_cap as u64) };
    arr
}
