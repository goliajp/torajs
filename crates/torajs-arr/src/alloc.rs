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

use torajs_rc::{AnySlotTag, FLAG_ARR_ANY, FLAG_SPLIT_BLOCK, FLAG_STATIC_LITERAL, HeapHeader};

use crate::any::{ANY_SLOT_BYTES, ANY_UNDEF, slot_anyvalue_ptr};
use crate::layout::{
    ARR_CELL_SIZE, ARR_DATA_PTR_OFF, ARR_LEN_OFF, ARR_PROPS_OFF, TAG_ARR, arr_data_is_inline,
};
use crate::pool::{POOL_CAP_MAX, POOL_SLOTS, pop_cap_match, push};

/// Head-offset slot (matches `any.rs`).
const ARR_HEAD_OFF: usize = 20;

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

/// Block size for cap-N regular `Array<T>`: 40-byte cell + 8 bytes
/// per inline slot.
#[inline]
fn block_size_regular(cap: u64) -> usize {
    ARR_CELL_SIZE + (cap as usize) * 8
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
        // Round 4 chunk 5a — inline props_dynobj slot initialized to
        // NULL. Set by arrprops_set (chunk 5b+) on first `arr.x = v`.
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        // B1 — data pointer starts self-referential (inline slots).
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
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
        // Grown arrays spilled slots to an external buffer — release
        // it first (B1). The cell itself must NOT enter the pool: its
        // cap field describes the buffer, not the cell's inline
        // region, so a cap-keyed pool reuse would under-allocate.
        if !unsafe { arr_data_is_inline(p as *const u8) } {
            unsafe { free(crate::layout::arr_data(p as *const u8) as *mut c_void) };
            unsafe { free(p) };
            return;
        }
        // Read cap (low 32 bits at offset 16). Pool only accepts
        // regular Array<T> — Array<Any> blocks bypass it (historic
        // stride-mismatch rule, kept as-is).
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

// ============================================================
// Array<Any> allocs — moved from `any.rs` (chunk 624 file-size
// split). Same malloc-only path: Any-arrays bypass the cap-matched
// pool (8-byte NaN-box stride vs the pool's typed-slot ledger).
// ============================================================

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box pack (undefined fill).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// Cross-tier — torajs-anyvalue NaN-box tag probe (I64/F64
    /// dispatch for the `new Array(any)` runtime type test).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    /// Cross-tier — legacy raw-value decode. Only called on the
    /// number tags here (ShortStr materializes under it — the
    /// element path stores the AnyValue bits untouched instead).
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-rc. NaN-box-safe: no-op for immediates,
    /// bumps the wrapped heap pointer's refcount for cells.
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — torajs-throw catchable RangeError.
    fn __torajs_throw_range_error(msg: *const u8);
}

#[inline]
pub(crate) unsafe fn write_header_any(p: *mut u8, len: u64, cap: u32) {
    unsafe {
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_ARR;
        *(p.add(6) as *mut u16) = FLAG_ARR_ANY;
        *(p.add(ARR_LEN_OFF) as *mut u64) = len;
        *(p.add(ARR_CAP_LOW32_OFF) as *mut u32) = cap;
        *(p.add(ARR_HEAD_OFF) as *mut u32) = 0; // Any-arrays never deque-shift
        // Round 4 chunk 5a — inline props_dynobj slot initialized to NULL.
        *(p.add(ARR_PROPS_OFF) as *mut u64) = 0;
        // B1 — data pointer starts self-referential (inline slots).
        *(p.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = p.add(ARR_CELL_SIZE);
    }
}

/// `__torajs_arr_alloc_any(cap)` — fresh empty Array<Any>.
/// Bypasses the regular Array<T> pool (different slot stride).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any(cap: u64) -> *mut u8 {
    unsafe {
        let total = ARR_CELL_SIZE + (cap as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, 0, cap as u32);
        p
    }
}

/// `__torajs_arr_alloc_any_filled(n)` — `new Array(n)` per ES spec
/// §23.1.2.1. len=cap=n, all slots boxed `ANY_UNDEF` so `arr[i]`
/// decodes as `undefined` per ES §10.4.2.1 (sparse missing-index
/// semantics densely emulated; true sparse hole would need an
/// elem-kind-tag substrate, L3b). Mirrors the hole-fill pattern in
/// `__torajs_arr_set_at_any`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8 {
    // §23.1.2.1 step 4.b — a length outside [0, 2^32-1] is a
    // RangeError (test262-bug-corpus RC-4 F5: `new Array(-1)` arrives
    // as 0xFFFF..FF u64 and SIGSEGVd through the overflowed malloc).
    // Message matches bun/JSC for uncaught-print byte parity. The
    // throw helper RETURNS; ssa_lower's emit_throw_check right after
    // the call diverts before the NULL is touched.
    const MAX_ARRAY_LEN: u64 = u32::MAX as u64;
    if n > MAX_ARRAY_LEN {
        unsafe {
            __torajs_throw_range_error(
                b"Array length must be a positive integer of safe magnitude.\0".as_ptr(),
            );
        }
        return core::ptr::null_mut();
    }
    unsafe {
        let total = ARR_CELL_SIZE + (n as usize) * ANY_SLOT_BYTES;
        let p = malloc(total) as *mut u8;
        write_header_any(p, n, n as u32);
        if n > 0 {
            let undef = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
            for k in 0..n {
                *slot_anyvalue_ptr(p, k) = undef;
            }
        }
        p
    }
}

/// `__torajs_arr_alloc_any_filled_f64(len)` — `new Array(len)` for a
/// Number operand that is not provably an integer at compile time.
/// §23.1.2.1 step 4.b: a len with ToUint32(len) != len (NaN /
/// ±Infinity / fractional / negative / > 2^32-1) is a RangeError.
/// The f64 entry exists because the SSA i64 coercion (FpToSi / const
/// fold) erases exactly the bits this check needs; integer-provable
/// operands stay on the u64 entry above. The round-trip cast check
/// avoids libm (`trunc`) — `as u64` saturates NaN/negative to 0 and
/// overflow to u64::MAX, all of which fail the equality or the range
/// guards. `-0.0` passes (`-0.0 >= 0.0`, round-trips to `0.0`) per
/// the spec's numeric (not SameValue) comparison.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc_any_filled_f64(len: f64) -> *mut u8 {
    const MAX_ARRAY_LEN: f64 = u32::MAX as f64;
    if !(len >= 0.0 && len <= MAX_ARRAY_LEN && (len as u64) as f64 == len) {
        unsafe {
            __torajs_throw_range_error(
                b"Array length must be a positive integer of safe magnitude.\0".as_ptr(),
            );
        }
        return core::ptr::null_mut();
    }
    unsafe { __torajs_arr_alloc_any_filled(len as u64) }
}

/// `__torajs_arr_new_from_any(v)` — `new Array(len)` for an Any
/// operand. §23.1.1.1 step 4 branches on whether the argument IS a
/// Number, not on coercing it to one: `new Array('3')` is `['3']`
/// (length 1) while `new Array(3)` is length 3, so the Any lane is
/// a runtime type test. Number tags reuse the validated length
/// entries above (a negative i64 arrives as an out-of-range u64;
/// the f64 entry keeps the NaN / ±Infinity / fractional bits the
/// step-4.b RangeError needs). Every other tag allocates a
/// length-1 Array<Any> holding the value. The box is borrowed:
/// the slot takes its own +1 via the NaN-box-safe rc_inc (no-op
/// for immediates), and `unbox_value` is never called on this
/// path — ShortStr stays inline instead of materializing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_new_from_any(v: u64) -> *mut u8 {
    unsafe {
        let tag = __torajs_anyv_unbox_tag(v);
        if tag == AnySlotTag::I64 as i64 {
            return __torajs_arr_alloc_any_filled(__torajs_anyv_unbox_value(v) as u64);
        }
        if tag == AnySlotTag::F64 as i64 {
            return __torajs_arr_alloc_any_filled_f64(f64::from_bits(
                __torajs_anyv_unbox_value(v) as u64
            ));
        }
        let p = __torajs_arr_alloc_any_filled(1);
        *slot_anyvalue_ptr(p, 0) = v;
        __torajs_rc_inc(v as *mut c_void);
        p
    }
}
