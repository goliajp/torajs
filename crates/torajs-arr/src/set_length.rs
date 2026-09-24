//! `arr.length = N` — the length write, both lanes (RFC 20260712
//! ArraySetLength backlog item; split from `grow.rs`, which had reached
//! the 500-line cap, in rotation 624). `grow.rs` keeps the storage
//! side — buffer growth, the hole-marking grow tail, push / shift /
//! reserve; this file keeps the WRITE: the §10.4.2.4 ToUint32 /
//! ToNumber validation, the integrity-level refusal both lanes share,
//! the typed scalar lane's truncate and the any lane's full resize.

use core::ffi::c_void;

use crate::grow::{ARR_HDR_CAP_OFF, ARR_HDR_HEAD_OFF, ARR_HDR_LEN_OFF, grow_slots_as_holes};
use crate::layout::arr_data;

unsafe extern "C" {
    /// torajs-throw — RangeError / TypeError sinks + pending probe.
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-anyvalue — NaN-box a (tag, value) pair / ToNumber.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_to_number(v: u64) -> f64;
    /// torajs-value-drop — release a truncated slot's cell.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `arr.length = v` — the refusal both lanes share, in spec order.
/// §10.1.9.2 OrdinarySet: a non-writable `length` (a freeze, or a
/// `defineProperty` lock) refuses EVERY write, the same value
/// included — before this the same-value write returned early and a
/// frozen array's `xs.length = xs.length` was silently legal. A seal
/// keeps `length` writable and refuses only a shrink, because
/// §10.4.2.4 step 15 deletes the tail and a sealed element cannot be
/// deleted; bun words the two refusals differently. Records the
/// TypeError and answers `true` when the write must not happen.
fn length_write_refuses(flags: u16, old_len: u64, new_len: u64) -> bool {
    length_write_refuses_flavored(flags, old_len, new_len, true)
}

/// [`length_write_refuses`] with the §28.1.13 Reflect.set flavor: the
/// same decision, recorded as a TypeError only when `throw_on_refusal`.
fn length_write_refuses_flavored(
    flags: u16,
    old_len: u64,
    new_len: u64,
    throw_on_refusal: bool,
) -> bool {
    if flags & (torajs_rc::FLAG_ARR_LENGTH_RO | torajs_rc::FLAG_FROZEN) != 0 {
        if throw_on_refusal {
            unsafe { __torajs_throw_type_error(c"Array length is not writable".as_ptr()) };
        }
        return true;
    }
    if new_len < old_len && flags & torajs_rc::FLAG_SEALED != 0 {
        if throw_on_refusal {
            unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
        }
        return true;
    }
    false
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
    // Chunk D (RFC 20260712-arr-exotic-define) — locked length
    // rejects every change on the scalar lane too.
    // A frozen array's `length` is non-writable (freeze sets it so), and
    // a SHRINK deletes the tail, which a seal forbids too — node/bun
    // throw on both with the array untouched, while a seal still lets
    // `length` grow. Before this the frozen spelling shrank and grew
    // silently ([3,1,2] → [3] / [3,1,2,null,null]).
    let flags = unsafe { *(arr.add(6) as *const u16) };
    if length_write_refuses(flags, old_len, new_n) {
        return;
    }
    if new_n == old_len {
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
    unsafe { set_length_any_impl(arr, tag, value, true) };
}

/// [`__torajs_arr_set_length_any`]'s §28.1.13 Reflect.set flavor — a
/// refusal (non-writable length, a seal's shrink, a non-configurable
/// index stopping the shrink) answers 0 instead of recording the
/// TypeError; a write answers 1. The RangeError of an invalid length
/// is not a refusal and records in both flavors. Same pair shape as
/// `__torajs_dynobj_set` / `_soft`. Before this `Reflect.set(sealed,
/// "length", 0)` threw where it must answer false.
///
/// # Safety
/// Same contract as [`__torajs_arr_set_length_any`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_any_soft(
    arr: *mut u8,
    tag: i64,
    value: i64,
) -> i64 {
    unsafe { set_length_any_impl(arr, tag, value, false) }
}

/// Flavor-carrying core of the two shells above; 1 = written (or
/// nothing to write), 0 = refused.
unsafe fn set_length_any_impl(arr: *mut u8, tag: i64, value: i64, throw_on_refusal: bool) -> i64 {
    // §10.4.2.4 steps 3-5 (刀 6 G9c, `define_length` twin) —
    // `newLen = ToUint32(v)` then `numberLen = ToNumber(v)`: the spec
    // really runs a side-effecting valueOf TWICE (observable hook
    // count), RangeError when the two disagree. A throwing valueOf
    // wins outright; the caller's throw check propagates.
    let anyv = unsafe { __torajs_anyv_box_from_pair(tag, value) };
    let n1 = unsafe { __torajs_anyv_to_number(anyv) };
    if unsafe { __torajs_throw_check() } != 0 {
        return 0;
    }
    let n2 = unsafe { __torajs_anyv_to_number(anyv) };
    if unsafe { __torajs_throw_check() } != 0 {
        return 0;
    }
    let new_len = crate::define_length::to_uint32(n1);
    if new_len as f64 != n2 {
        unsafe {
            __torajs_throw_range_error(b"Invalid array length\0".as_ptr());
        }
        return 0;
    }
    let new_n = new_len as u64;
    let len_ptr = unsafe { arr.add(ARR_HDR_LEN_OFF) as *mut u64 };
    let old = unsafe { *len_ptr };
    let header = unsafe { &*(arr as *const torajs_rc::HeapHeader) };
    // Chunk D — same rule as the scalar lane above.
    if length_write_refuses_flavored(header.flags, old, new_n, throw_on_refusal) {
        return 0;
    }
    if new_n == old {
        return 1;
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
        // §10.4.2.4 step 19 — a non-configurable index stopped the
        // walk short of the requested length: the write landed as far
        // as it could and the operation answers false. The strict
        // assignment's TypeError is NOT recorded here: the kernel
        // cannot tell a sloppy program's write (silent, §13.15.2 with
        // strict false) from a strict one, and test262's sloppy
        // `indexOf/15.4.4.14-9-a-19` shrinks exactly this way inside a
        // getter — the assignment path keeps its silent landing
        // (registered: strict / sloppy [[Set]] refusal flavors).
        if land != new_n {
            return 0;
        }
        return 1;
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
        return 1;
    }
    // Grow — every kind grows now (RFC 20260721 刀 5 G3): NaN-box
    // slots fill with the undefined immediate (0x0A), raw-kind slots
    // with 0; either way each new index is marked a HOLE, so the
    // fill value is never an own property answer.
    let fill = if is_any { 0x0A } else { 0 };
    unsafe { grow_slots_as_holes(arr, old, new_n, fill) };
    1
}
