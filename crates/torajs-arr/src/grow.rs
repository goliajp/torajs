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

use crate::layout::{ARR_DATA_PTR_OFF, arr_data, arr_data_is_inline};

unsafe extern "C" {
    /// Cross-tier — provided by torajs-throw at `tr build` link time
    /// via `libtorajs_throw.a`.
    ///
    /// **Returns normally** — does NOT longjmp / panic. Internally
    /// records the pending throw via TLS (`__torajs_throw_set`). The
    /// caller's SSA-emitted `emit_throw_check` after our `return` is
    /// what actually propagates to user-side `try/catch`.
    fn __torajs_throw_range_error(msg: *const u8);

    /// Cross-tier — torajs-throw catchable TypeError (RFC
    /// 20260712-arr-exotic-define chunk D length lock).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);

    /// Cross-tier — torajs-throw pending-throw probe (刀 6 G9c: a
    /// throwing valueOf aborts between the spec's two conversions).
    fn __torajs_throw_check() -> i64;

    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_libc_realloc"]
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;

    /// torajs-mmalloc libc-compat malloc — first grow spills the
    /// inline slots region to an independent buffer (B1).
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;

    /// torajs-anyvalue — NaN-box a `(tag, value)` pair.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-anyvalue — spec ToNumber over a NaN-box AnyValue
    /// (`"2"` parses, `true` = 1, `null` = 0, `undefined` = NaN).
    fn __torajs_anyv_to_number(v: u64) -> f64;
    /// Cross-tier — universal heap value dropper (NaN-box safe; the
    /// cell gate skips immediates).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// RFC 20260706-arr-grow-alias-stability B1 — grow the data buffer to
/// `new_cap` slots (8-byte stride; typed and Array<Any> share it).
/// The CELL never moves, so every alias — caller bindings, container
/// slots, struct fields, globals — stays valid across grow. First
/// grow spills the inline region to a malloc'd buffer (full physical
/// range copied, preserving deque head layout); later grows realloc
/// the buffer in place.
pub(crate) unsafe fn grow_data_buffer(arr: *mut u8, new_cap: u64) {
    unsafe {
        let new_bytes = (new_cap as usize) * 8;
        let old_data = arr_data(arr);
        let new_data = if arr_data_is_inline(arr) {
            let buf = malloc(new_bytes) as *mut u8;
            let old_cap = *(arr.add(ARR_HDR_CAP_OFF) as *const u32) as usize;
            core::ptr::copy_nonoverlapping(old_data, buf, old_cap * 8);
            buf
        } else {
            realloc(old_data as *mut c_void, new_bytes) as *mut u8
        };
        *(arr.add(ARR_DATA_PTR_OFF) as *mut *mut u8) = new_data;
        *(arr.add(ARR_HDR_CAP_OFF) as *mut u32) = new_cap as u32;
    }
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
            // 刀 6 G9c — a heap operand runs the spec's §10.4.2.4
            // ToUint32 + ToNumber pair (valueOf observably twice,
            // RangeError on disagreement), same as the any-lane twin
            // below.
            let anyv = unsafe { __torajs_anyv_box_from_pair(tag, value) };
            let n1 = unsafe { __torajs_anyv_to_number(anyv) };
            if unsafe { __torajs_throw_check() } != 0 {
                return;
            }
            let n2 = unsafe { __torajs_anyv_to_number(anyv) };
            if unsafe { __torajs_throw_check() } != 0 {
                return;
            }
            let nl = crate::define_length::to_uint32(n1);
            if nl as f64 != n2 {
                unsafe {
                    __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
                }
                return;
            }
            nl as f64
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
    if new_n == old_len {
        return;
    }
    // Chunk D (RFC 20260712-arr-exotic-define) — locked length
    // rejects every change on the scalar lane too.
    let flags = unsafe { *(arr.add(6) as *const u16) };
    if flags & torajs_rc::FLAG_ARR_LENGTH_RO != 0 {
        unsafe { __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr()) };
        return;
    }
    if new_n < old_len {
        unsafe { *len_ptr = new_n };
        return;
    }
    // Grow (RFC 20260721 刀 5 G3) — raw-0 fill, every new index a
    // HOLE (not an own property; reads through the any tier consult
    // the prototype digit keys). The typed STATIC load keeps its
    // raw-0 blindness — recorded L3b.
    unsafe { grow_slots_as_holes(arr, old_len, new_n, 0) };
}

/// Shared §10.4.2.5 length-grow tail — compact the deque head, grow
/// the buffer, fill `[old, new_n)` with `fill`, write `len`, and mark
/// every new index a HOLE (RFC 20260721 刀 5 G3: a grown slot is NOT
/// an own property — `1 in x` after `x.length = 2` is false and
/// element reads continue to the prototype digit keys).
unsafe fn grow_slots_as_holes(arr: *mut u8, old: u64, new_n: u64, fill: u64) {
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
///   if len == cap: grow_data_buffer(max(4, cap*2))
/// store: *(arr_data(arr) + (head + len)*8) = val; len += 1
/// ```
///
/// # Safety
/// `extern "C"` ABI. `arr` must be a live Array<T> heap block (8-byte
/// slot stride — Array<Any> has its own push_any path in
/// [`crate::any`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8 {
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
        if len == cap {
            let new_cap = if cap == 0 { 4 } else { cap * 2 };
            unsafe { grow_data_buffer(arr, new_cap as u64) };
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
        let head_p = arr.add(ARR_HDR_HEAD_OFF) as *mut u32;
        let head = *head_p as usize;
        let slot = arr_data(arr).add(head * 8) as *const i64;
        let v = *slot;
        *head_p = (head + 1) as u32;
        let len_p = arr.add(ARR_HDR_LEN_OFF) as *mut u64;
        *len_p -= 1;
        v
    }
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

/// `arr.length = N` / `defineProperty(arr, "length", { value: N })`
/// with a real resize — spec §10.4.2.5 ArraySetLength (RFC
/// 20260712-object-create-define-props backlog item). ToNumber goes
/// through the NaN-box coercer (`"2"` parses, `true` = 1, `null` = 0,
/// `undefined` = NaN); ToUint32 must equal ToNumber, else RangeError.
///
/// - `N < len` — release each removed slot's cell (NaN-box walk for
///   `Array<Any>`, raw-cell walk for HEAP-kind typed blocks; scalar
///   slots carry no refs), then write `len = N`.
/// - `N > len` — compact the deque head, grow the data buffer, fill
///   `[len, N)` (NaN-box undefined for `Array<Any>`, raw 0 for typed
///   kinds) and mark every new index a HOLE (RFC 20260721 刀 5 G3 —
///   a grown slot is not an own property).
///
/// # Safety
/// `arr` is a live array heap block; `(tag, value)` is a valid
/// AnySlotTag pair. Caller must check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_any(arr: *mut u8, tag: i64, value: i64) {
    // §10.4.2.4 steps 3-5 (刀 6 G9c, `define_length` twin) —
    // `newLen = ToUint32(v)` then `numberLen = ToNumber(v)`: the spec
    // really runs a side-effecting valueOf TWICE (observable hook
    // count), RangeError when the two disagree. A throwing valueOf
    // wins outright; the caller's throw check propagates.
    let anyv = unsafe { __torajs_anyv_box_from_pair(tag, value) };
    let n1 = unsafe { __torajs_anyv_to_number(anyv) };
    if unsafe { __torajs_throw_check() } != 0 {
        return;
    }
    let n2 = unsafe { __torajs_anyv_to_number(anyv) };
    if unsafe { __torajs_throw_check() } != 0 {
        return;
    }
    let new_len = crate::define_length::to_uint32(n1);
    if new_len as f64 != n2 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
        return;
    }
    let new_n = new_len as u64;
    let len_ptr = unsafe { arr.add(ARR_HDR_LEN_OFF) as *mut u64 };
    let old = unsafe { *len_ptr };
    if new_n == old {
        return;
    }
    let header = unsafe { &*(arr as *const torajs_rc::HeapHeader) };
    // Chunk D — a locked length (defineProperty writable: false)
    // rejects every change; the same-value path above stays legal
    // (SameValue redefine is not a change per §10.1.6.3).
    if header.flags & torajs_rc::FLAG_ARR_LENGTH_RO != 0 {
        unsafe { __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr()) };
        return;
    }
    let is_any = header.flags & torajs_rc::FLAG_ARR_ANY != 0;
    let head = unsafe { *(arr.add(ARR_HDR_HEAD_OFF) as *const u32) } as usize;
    if new_n < old {
        // §10.4.2.4 steps 15-19 shadow sweep (刀 13d) — truncated
        // indexes' shadow entries (accessor pairs / hole sentinels)
        // die with the elements; a live non-configurable entry stops
        // the walk and the shrink lands at stop+1 (silent
        // assignment-path semantics; `define_length` pre-validates
        // the throwing define path).
        let land = if header.flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
            unsafe { crate::define_hole::shrink_purge_shadows(arr as *mut c_void, new_n, old) }
        } else {
            new_n
        };
        // §10.4.2.5 step 4 — the removed slots' refs die here (this
        // is the only owner walk; value_drop_heap's cell gate skips
        // immediates / raw scalars stay unwalked). Bounded by the
        // materialized extent — a sparse tail (RFC
        // 20260810-arr-sparse-grow) has no slots to walk.
        let drop_hi = core::cmp::min(old, unsafe { crate::layout::arr_live_extent(arr) });
        if is_any || header.arr_elem_kind() == torajs_rc::ARR_KIND_HEAP {
            let slots = unsafe { arr_data(arr) };
            for i in land..drop_hi {
                let av = unsafe { *(slots.add((head + i as usize) * 8) as *const u64) };
                unsafe { __torajs_value_drop_heap(av as *mut c_void) };
            }
        }
        unsafe { *len_ptr = land };
        // RFC 20260810 刀 E — a shrink that lands inside the
        // materialized extent leaves every remaining index with a
        // slot behind it: the cell is dense again, the flag drops
        // (its explicit-hole shadows persist independently).
        if header.flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0 {
            let cap = unsafe { *(arr.add(ARR_HDR_CAP_OFF) as *const u32) } as u64;
            if land <= cap {
                unsafe {
                    (*(arr as *mut torajs_rc::HeapHeader)).flags &=
                        !torajs_rc::FLAG_ARR_SPARSE_TAIL;
                }
            }
        }
        return;
    }
    // RFC 20260810 刀 E — a giant grow on an `Array<Any>` cell goes
    // SPARSE instead of materializing: `len` moves, `cap` stays the
    // materialized extent (`cap == extent` invariant; Any-arrays
    // never deque-shift so head is 0), and `[extent, len)` becomes
    // implicit holes with no storage and no shadow entries — O(1)
    // where the fill below is O(new_n) time and memory (the
    // `a.length = 4294967295` shape was a 34GB materialization).
    // Below the limit the historical materializing grow keeps its
    // exact semantics; typed cells always materialize (their static
    // emit lanes never see a sparse cell).
    if is_any && new_n > crate::any::ARR_DENSE_LIMIT {
        let already_sparse = header.flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0;
        if !already_sparse {
            // `cap` becomes the extent ledger: every index in
            // `[0, old)` has a slot (cap >= len always holds for a
            // dense cell), so the extent is exactly `old`.
            unsafe { *(arr.add(ARR_HDR_CAP_OFF) as *mut u32) = old as u32 };
            unsafe {
                (*(arr as *mut torajs_rc::HeapHeader)).flags |= torajs_rc::FLAG_ARR_SPARSE_TAIL;
            }
        }
        // Already-sparse: `cap` is the live extent — only `len`
        // moves.
        unsafe { *len_ptr = new_n };
        return;
    }
    // Grow — every kind grows now (RFC 20260721 刀 5 G3): NaN-box
    // slots fill with the undefined immediate (0x0A), raw-kind slots
    // with 0; either way each new index is marked a HOLE, so the
    // fill value is never an own property answer.
    let fill = if is_any { 0x0A } else { 0 };
    unsafe { grow_slots_as_holes(arr, old, new_n, fill) };
}
