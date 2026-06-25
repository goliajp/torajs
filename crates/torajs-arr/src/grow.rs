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
const ARR_HDR_LEN_OFF: usize = 8;

/// Offset of the `cap` u32 within an array heap block. T-13.5 packed
/// cap (u32) + head_offset (u32) into the 8-byte slot at offset 16
/// (formerly cap was a u64). Mirrors ssa_lower's deque-layout table.
const ARR_HDR_CAP_OFF: usize = 16;

/// Offset of the `head_offset` u32 within an array heap block (T-13.5
/// deque packed cap + head).
const ARR_HDR_HEAD_OFF: usize = 20;

/// Offset of the slot array within an array heap block. Mirrors
/// `crate::layout::ARR_SLOTS_OFF` (single source of truth) — bumped
/// 24 → 32 in Round 4 chunk 5a so the inline `props_dynobj` slot
/// fits between cap/head and slots. ssa_lower's deque-layout table
/// is kept in sync via `torajs_core::ssa_lower::ARR_DATA_OFF`.
use crate::layout::ARR_SLOTS_OFF as ARR_HDR_DATA_OFF;

unsafe extern "C" {
    /// Cross-tier — provided by torajs-throw at `tr build` link time
    /// via `libtorajs_throw.a`.
    ///
    /// **Returns normally** — does NOT longjmp / panic. Internally
    /// records the pending throw via TLS (`__torajs_throw_set`). The
    /// caller's SSA-emitted `emit_throw_check` after our `return` is
    /// what actually propagates to user-side `try/catch`.
    fn __torajs_throw_range_error(msg: *const u8);

    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_realloc"]
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;

    /// torajs-mmalloc libc-compat malloc / free — used by the pool-aware
    /// grow path in `__torajs_arr_push` (alloc-new + memcpy + free-old
    /// replaces `realloc` so cap=0 blocks re-enter the array pool —
    /// Round 4 wire-back chunk 2 attack #4, 2026-06-25).
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// `arr.length = v` validator (ES §9.4.2.4: throw RangeError if `v`
/// doesn't ToUint32-round-trip).
///
/// Tora's typed pack:
/// - tag 0 = null/other → ToNumber=0 → valid (early return)
/// - tag 1 = Bool 0/1 → valid (early return)
/// - tag 2 = I64 → interpret raw int as length candidate
/// - tag 3 = F64-bits → reinterpret raw bits as f64
/// - other = heap / undefined → record RangeError + return
///
/// Range check: `n` must be a non-negative integer in `[0, 2^32 - 1]`.
/// NaN, Infinity, fractional, negative, and overflow all fail.
///
/// After every `__torajs_throw_range_error` call we `return` so the
/// caller's `emit_throw_check` sees the pending throw immediately (the
/// throw is non-local via TLS, not via stack unwind — see fn-level
/// extern doc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_validate(tag: i64, value: i64) {
    let n: f64 = match tag {
        0 | 1 => return,
        2 => value as f64,
        3 => f64::from_bits(value as u64),
        _ => {
            unsafe {
                __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
            }
            return;
        }
    };
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != (n as i64) as f64 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
    }
}

/// `arr.length = N` truncate path for `Array<scalar>` (I64 / F64 / Bool
/// element types — no per-slot drop required). Combines the
/// `__torajs_arr_set_length_validate` RangeError gate with an actual
/// `len` write so spec §10.4.2.5 step 4 (delete elements `[N, oldLen)`)
/// is honored instead of being a silent no-op.
///
/// Semantics:
/// - Invalid `N` (NaN / fractional / negative / overflow): record
///   RangeError via `__torajs_throw_range_error`, do not touch `len`.
/// - Valid `N <= old_len`: write `len = N`. Backing storage is kept
///   (matches V8 / JSC behavior — `cap` doesn't shrink on truncate).
/// - Valid `N > old_len`: silent no-op. typed `Array<scalar>` can't
///   represent the "hole / undefined" the spec wants there; the
///   `Array<Any>` extend path is a substrate-mid follow-up.
///
/// # Safety
/// `arr` must be a live `Array<T>` heap block. Caller picks this
/// helper only when the SSA-known element type is a non-refcounted
/// scalar (I64 / F64 / Bool).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_truncate_scalar(
    arr: *mut u8,
    tag: i64,
    value: i64,
) {
    let n: f64 = match tag {
        0 | 1 => 0.0,
        2 => value as f64,
        3 => f64::from_bits(value as u64),
        _ => {
            unsafe {
                __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
            }
            return;
        }
    };
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != (n as i64) as f64 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
        return;
    }
    let new_n = n as u64;
    let len_ptr = unsafe { arr.add(ARR_HDR_LEN_OFF) as *mut u64 };
    let old_len = unsafe { *len_ptr };
    if new_n < old_len {
        unsafe { *len_ptr = new_n };
    }
    // new_n >= old_len: scalar typed array can't extend with undefined
    // — leave len untouched (silent no-op for now, recorded as L3b).
}

/// Push `val` onto the end of `arr`, growing the backing block if
/// needed. Returns the (possibly relocated) array pointer; caller
/// stores it back. Mirrors `__torajs_arr_push_unchecked` semantically
/// but with the cap-check + compact + grow path.
///
/// Algorithm (1:1 port of ssa_inkwell::define_arr_push, 187 LOC IR @
/// L2647-2829; collapsed via native realloc + ptr::copy + linear
/// control flow):
///
/// ```text
/// fast path: head + len < cap → store immediately
/// need-room:
///   if head > 0: memmove(data, data + head*8, len*8); head = 0  // T-13.5 compact
///   if len == cap: realloc(new_cap = max(4, cap*2)); update cap
/// store: data = arr + 24 + head*8 (re-load head); *(data + len*8) = val; len += 1
/// ```
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live Array<T> heap block (8-byte
/// slot stride — Array<Any> uses a 16-byte stride and has its own
/// push_any path in [`crate::any`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8 {
    let len = unsafe { *(arr.add(ARR_HDR_LEN_OFF) as *const u64) } as i64;
    let cap = unsafe { *(arr.add(ARR_HDR_CAP_OFF) as *const u32) } as i64;
    let head = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as i64;
    let phys_used = head + len;

    let mut arr = arr;
    if phys_used >= cap {
        if head > 0 {
            unsafe {
                let raw_data = arr.add(ARR_HDR_DATA_OFF);
                let src = raw_data.add((head as usize) * 8);
                core::ptr::copy(src, raw_data, (len as usize) * 8);
                *(arr.add(ARR_HDR_HEAD_OFF) as *mut u32) = 0;
            }
        }
        if len == cap {
            let new_cap = if cap == 0 { 4 } else { cap * 2 };
            let new_total = (new_cap as usize) * 8 + ARR_HDR_DATA_OFF;
            // Pool-aware grow (Round 4 wire-back chunk 2 attack #4):
            // alloc new (pool-first) + memcpy + free old (pool-first).
            // Replaces `realloc` so the freed cap=0 / cap=N block re-enters
            // the cap-match pool — without this, the fast `[].push(x)`
            // pattern realloc's the cap=0 block in-place, leaving alloc(0)
            // forever pool-miss on subsequent iterations.
            let old_arr = arr;
            let new_arr: *mut u8 = if (new_cap as u64) <= crate::pool::POOL_CAP_MAX {
                let r = crate::pool::pop_cap_match(new_cap as u64);
                if !r.is_null() {
                    r
                } else {
                    unsafe { malloc(new_total) as *mut u8 }
                }
            } else {
                unsafe { malloc(new_total) as *mut u8 }
            };
            // head was compacted to 0 immediately above; copy header (24B)
            // + live slots [0..len) only.
            let copy_total = ARR_HDR_DATA_OFF + (len as usize) * 8;
            unsafe { core::ptr::copy_nonoverlapping(old_arr, new_arr, copy_total) };
            arr = new_arr;
            unsafe {
                *(arr.add(ARR_HDR_CAP_OFF) as *mut u32) = new_cap as u32;
            }
            // Free old: regular Array<T> reaching arr_push cannot carry
            // FLAG_ARR_ANY (uses arr_push_any) / FLAG_SPLIT_BLOCK (split
            // result is immutable) / FLAG_STATIC_LITERAL (would COW
            // upstream). Push directly to the cap-match pool; libc free
            // on pool full.
            let count = crate::pool::current_count();
            let pushed = (cap as u64) <= crate::pool::POOL_CAP_MAX
                && count < crate::pool::POOL_SLOTS
                && crate::pool::push(old_arr, cap as u64);
            if !pushed {
                unsafe { free(old_arr as *mut c_void) };
            }
        }
    }

    // Re-load head — compact path may have reset it to 0.
    let head_now = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as i64;
    unsafe {
        let data = arr.add(ARR_HDR_DATA_OFF + (head_now as usize) * 8);
        let slot = data.add((len as usize) * 8) as *mut i64;
        *slot = val;
        *(arr.add(ARR_HDR_LEN_OFF) as *mut u64) = (len + 1) as u64;
    }
    arr
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
/// bug-327 C3 — out-of-bounds typed indexed write rejector. The
/// typed-tier `arr[i] = v` emit guards the inline slot store with an
/// `i < len` branch; the OOB path calls this to raise a catchable
/// RangeError (pre-fix the unchecked StoreDyn silently corrupted
/// adjacent heap for small `i` and SIGSEGV'd past the page). Typed
/// OOB-grow semantics (length extension + zero fill) stay a roadmap
/// item — RFC 20260613-test262-bug327-root-causes tracks it; the
/// untyped Array<Any> path grows for real via
/// `__torajs_arr_set_any_grow`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_oob_write_reject(_i: i64) {
    unsafe {
        __torajs_throw_range_error(
            b"out-of-bounds typed-array index write (typed grow-on-write is not yet supported)\0"
                .as_ptr(),
        );
    }
}

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
        let head_p = arr.add(ARR_HDR_HEAD_OFF) as *mut u32;
        let head = *head_p as usize;
        let slot = arr.add(ARR_HDR_DATA_OFF + head * 8) as *const i64;
        let v = *slot;
        *head_p = (head + 1) as u32;
        let len_p = arr.add(ARR_HDR_LEN_OFF) as *mut u64;
        *len_p -= 1;
        v
    }
}

/// Grow an array's backing block to fit at least `new_cap` elements.
/// Cap-equal short-circuits to no-op (returns input pointer unchanged).
///
/// **Returns the (possibly relocated) array pointer** — the caller
/// must use the return value, not the input pointer, since `realloc`
/// may move the block.
///
/// Algorithm (1:1 port of ssa_inkwell::define_arr_reserve, 66 LOC IR
/// → ~10 LOC Rust thanks to native realloc + raw-pointer arithmetic):
///
/// ```text
/// if cap(arr) >= new_cap: return arr   // no-op short-circuit
/// new_total = new_cap * 8 + ARR_HDR_DATA_OFF
/// arr = realloc(arr, new_total)
/// *(u32*)(arr + ARR_HDR_CAP_OFF) = new_cap as u32
/// return arr
/// ```
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live array heap block (non-NULL,
/// allocated via `__torajs_arr_alloc*`); `new_cap` non-negative.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_reserve(arr: *mut u8, new_cap: i64) -> *mut u8 {
    let cap_p = unsafe { arr.add(ARR_HDR_CAP_OFF) as *mut u32 };
    let cap = unsafe { *cap_p } as i64;
    if cap >= new_cap {
        return arr;
    }
    let new_total = (new_cap as usize) * 8 + ARR_HDR_DATA_OFF;
    let arr_grown = unsafe { realloc(arr as *mut c_void, new_total) as *mut u8 };
    unsafe {
        *(arr_grown.add(ARR_HDR_CAP_OFF) as *mut u32) = new_cap as u32;
    }
    arr_grown
}
