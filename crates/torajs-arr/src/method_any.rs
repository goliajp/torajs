//! `any`-receiver Array method glue (Any-method-call RFC 20260704 C1
//! + C2) — `a.push(v)` / `a.pop()` / `a.shift()` / `a.unshift(v)`
//! where `a` crossed into the `any` world. The read-only search /
//! join family lives in [`crate::method_any_search`].
//!
//! Called by `__torajs_any_method_call`'s Tag::Arr arm
//! (torajs-anyvalue) with already-unboxed receiver pointers; each
//! fn adapts one AnyValue-shaped argument surface onto the existing
//! typed/Any array kernels:
//!
//! - push → kind-matched raw store via the S3-set coercion rules
//!   (`index_any.rs` shapes) + the pool-aware growing
//!   [`crate::grow::__torajs_arr_push`] / Arr<Any>'s
//!   [`crate::any::__torajs_arr_push_any`]. Growth may relocate the
//!   block — the new pointer writes back through `recv_slot` (the
//!   caller's variable slot; NaN-box cell encoding is the raw
//!   pointer bits) when one exists.
//! - pop → reuse the kind-aware boxed read
//!   ([`crate::index_any::__torajs_arr_index_get`], balanced +1 for
//!   cells), release the vacated slot's own reference, shrink len.
//!
//! Reference ledger: arguments are BORROWED from the caller
//! (`ssa_lower_any_method_call` rc-decs every argv slot after the
//! call) — push incs what it stores. Returns follow the boxed-value
//! convention (cells +1 owned by the caller).

use core::ffi::c_void;

use torajs_rc::{
    __torajs_rc_inc, ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::layout::{ARR_LEN_OFF, arr_data};

const ARR_HEAD_OFF: usize = 20;

/// Sentinel for "a pending throw was recorded" — the dispatcher
/// stops a multi-argument push loop instead of writing past the
/// failure.
pub const ANY_METHOD_THREW: u64 = u64::MAX;

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue. `pop` / `shift` / `unshift`
    /// through the array-like arm's per-index [[Get]] / Delete / Set
    /// sequence (`argv` borrowed; NULL / 0 without arguments).
    fn __torajs_arr_mutate_generic(arr: *mut c_void, mid: i64, argv: *const u64, argc: i64) -> u64;
    fn __torajs_throw_check() -> i64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `a.push(…args)` — ES-variadic; each argument appends in order
/// through the kind-coercion table. Returns the new length as a raw
/// u64, or [`ANY_METHOD_THREW`] when a kind-mismatch TypeError was
/// recorded mid-loop (earlier arguments stay appended, matching the
/// ES step-by-step Set semantics).
///
/// Growth may relocate the block — the loop chases the fresh
/// pointer and writes it back through `recv_slot` (NaN-box cell
/// encoding is the raw pointer bits) so the caller's variable stays
/// current.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `argv` points at
/// `argc` live AnyValue slots (borrowed — this fn incs what it
/// stores); `recv_slot` is NULL or the receiver variable's live
/// slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_push(
    arr: *mut c_void,
    argv: *const u64,
    argc: i64,
    recv_slot: *mut u64,
) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — the append slot sits past the
        // materialized extent; loud reject until push grows real
        // sparse support.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.push\0".as_ptr(),
        ) {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // §23.1.3.23 step 6 — with nothing to append the only write is
        // `length` (the same value), which a frozen array or a
        // non-writable length refuses (Set with Throw) and a seal /
        // preventExtensions does not (rotation 652:
        // `Object.freeze([]).push()` answered 0, and the extend guard
        // below threw on `Object.seal([1]).push()`).
        if argc == 0 {
            if crate::define_length::__torajs_arr_len_write_guard(arr) != 0 {
                return __torajs_anyv_box_from_pair(5, 0);
            }
        }
        // Non-extensible receiver → TypeError before the first append
        // (`__torajs_arr_extend_guard`); `argv` is borrowed, so the
        // reject releases nothing. Checked here rather than left to
        // the per-element kernels so a multi-arg `push(7, 8)` records
        // one throw and stores nothing, on either block kind.
        else if crate::define_length::__torajs_arr_extend_guard(arr) != 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        let mut cur = arr as *mut u8;
        for i in 0..argc {
            let av = *argv.add(i as usize);
            let header = &*(cur as *const HeapHeader);
            if header.flags & FLAG_ARR_ANY != 0 {
                // Arr<Any> native ledger — store the box bits directly
                // (rotation 546 form: the pair spelling forced an
                // unbox that leaked one materialized Str per ShortStr
                // arg). The boxed entry takes its own +1 for cells.
                cur = crate::any::__torajs_arr_push_any_boxed(cur, av);
            } else {
                // Typed tier — same kind-coercion table as the
                // S3-set index write (`__torajs_arr_index_set`).
                let Some(raw) = coerce_typed_slot(header.arr_elem_kind(), av) else {
                    return kind_mismatch_threw(
                        c"push through an any array receiver would change the array's element kind",
                    );
                };
                cur = crate::grow::__torajs_arr_push(cur, raw as i64);
            }
            if !recv_slot.is_null() {
                *recv_slot = cur as u64;
            }
        }
        *(cur.add(ARR_LEN_OFF) as *const u64)
    }
}

/// Typed-tier `(kind, boxed AnyValue) → raw slot repr` coercion —
/// the S3-set table shared by push / unshift, taking the whole box
/// so the HEAP arm can tell a real cell (inc the pointer the box IS)
/// from a ShortStr (whose materialization carries the fresh rc=1
/// stake the slot adopts — rotation 546: the pair spelling
/// double-staked those). `None` = the value can't store without
/// changing the array's element kind (caller raises the TypeError);
/// a `Some` for a HEAP slot carries exactly one stored reference.
unsafe fn coerce_typed_slot(kind: u16, av: u64) -> Option<u64> {
    let tag = unsafe { __torajs_anyv_unbox_tag(av) };
    if tag == 4 {
        if kind != ARR_KIND_HEAP {
            return None;
        }
        if torajs_rc::ffi::nan_box_is_cell_like(av as *mut c_void) {
            unsafe { __torajs_rc_inc(av as *mut c_void) };
            return Some(av);
        }
        return Some(unsafe { __torajs_anyv_unbox_value(av) } as u64);
    }
    let value = unsafe { __torajs_anyv_unbox_value(av) };
    match (kind, tag) {
        (ARR_KIND_I64, 2) => Some(value as u64),
        (ARR_KIND_I64, 3) => {
            let d = f64::from_bits(value as u64);
            if d.fract() != 0.0 || !d.is_finite() {
                return None;
            }
            Some(d as i64 as u64)
        }
        (ARR_KIND_F64, 3) => Some(value as u64),
        (ARR_KIND_F64, 2) => Some((value as f64).to_bits()),
        (ARR_KIND_BOOL, 1) => Some(value as u64),
        _ => None,
    }
}

/// Record the kind-mismatch TypeError and answer the throw sentinel.
unsafe fn kind_mismatch_threw(msg: &core::ffi::CStr) -> u64 {
    unsafe {
        __torajs_throw_type_error(msg.as_ptr());
    }
    ANY_METHOD_THREW
}

/// `a.pop()` — boxed last element (cells +1, ownership to the
/// caller), `undefined` for an empty array. Shrinks len and releases
/// the vacated slot's own reference.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_pop(arr: *mut c_void) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — `len - 1` has no slot behind it; loud
        // reject until pop grows real sparse support.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.pop\0".as_ptr(),
        ) {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // A hole or an accessor index: the element's [[Get]] runs before
        // the length Set, and may be what makes the Set throw.
        if crate::layout::needs_index_mop(arr as *const u8) {
            return __torajs_arr_mutate_generic(
                arr,
                torajs_rc::ANY_METHOD_POP,
                core::ptr::null(),
                0,
            );
        }
        // RFC 20260713 blade 4 — frozen / RO-length receivers throw
        // before the empty short-circuit (§23.1.3.20 step 3.b); a
        // SEALED one too, since the popped element must be
        // configurable to be deleted (`__torajs_arr_delete_tail_guard`).
        if crate::define_length::__torajs_arr_delete_tail_guard(arr) != 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        let p = arr as *mut u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        let idx = (len - 1) as i64;
        // Kind-aware boxed read — balanced (+1 for cells).
        let out = crate::index_any::__torajs_arr_index_get(arr, idx);
        // Release the vacated slot's own reference (the read above
        // took a fresh one for the caller).
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY != 0 {
            // Arr<Any>: 8-byte NaN-box slots, no deque head (mirrors
            // `any::slot_anyvalue_ptr` / `drop::__torajs_arr_drop_any`).
            let slot = *(arr_data(p).add(((len - 1) as usize) * 8) as *const u64);
            __torajs_value_drop_heap(slot as *mut c_void);
        } else if header.arr_elem_kind() == ARR_KIND_HEAP {
            let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
            let slot = *(arr_data(p).add(((head + len - 1) as usize) * 8) as *const u64);
            __torajs_value_drop_heap(slot as *mut c_void);
        }
        *(p.add(ARR_LEN_OFF) as *mut u64) = len - 1;
        out
    }
}

/// `a.shift()` — boxed first element (cells +1, ownership to the
/// caller), `undefined` for an empty array. Typed tier bumps the
/// deque head ([`crate::grow::__torajs_arr_shift`]); Arr&lt;Any&gt;
/// never deque-shifts (head stays 0 by layout contract), so its
/// slots memmove forward. Either way the vacated slot's own
/// reference releases first.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_shift(arr: *mut c_void) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — the relocation walk crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.shift\0".as_ptr(),
        ) {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // pop's twin: the per-index walk for a hole / accessor index.
        if crate::layout::needs_index_mop(arr as *const u8) {
            return __torajs_arr_mutate_generic(
                arr,
                torajs_rc::ANY_METHOD_SHIFT,
                core::ptr::null(),
                0,
            );
        }
        // RFC 20260713 blade 4 — pop's twin; the lock raiser the typed
        // lane uses (a sealed receiver's partial move included, see
        // `sealed_refuse::__torajs_arr_shift_guard`).
        if crate::sealed_refuse::__torajs_arr_shift_guard(arr) != 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        let p = arr as *mut u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // Kind-aware boxed read of slot 0 — balanced (+1 for cells).
        let out = crate::index_any::__torajs_arr_index_get(arr, 0);
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY != 0 {
            let slot0 = *(arr_data(p) as *const u64);
            __torajs_value_drop_heap(slot0 as *mut c_void);
            core::ptr::copy(arr_data(p).add(8), arr_data(p), ((len - 1) as usize) * 8);
            *(p.add(ARR_LEN_OFF) as *mut u64) = len - 1;
        } else {
            if header.arr_elem_kind() == ARR_KIND_HEAP {
                let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
                let slot = *(arr_data(p).add((head as usize) * 8) as *const u64);
                __torajs_value_drop_heap(slot as *mut c_void);
            }
            // Deque-head bump + len shrink (the raw return is the
            // already-released slot value — ignore it).
            crate::grow::__torajs_arr_shift(p);
        }
        out
    }
}

/// `a.unshift(…args)` — ES-variadic prepend; `unshift(x, y)` yields
/// `[x, y, …rest]`, which the reverse-order single-value loop
/// reproduces. Returns the new length, or [`ANY_METHOD_THREW`] on a
/// typed-tier kind mismatch (earlier prepends stay, matching the
/// push arm's step-by-step semantics). Relocations write back
/// through `recv_slot`.
///
/// # Safety
/// `arr` is a valid `Tag::Arr` heap pointer; `argv` points at `argc`
/// live AnyValue slots (borrowed — this fn incs what it stores);
/// `recv_slot` is NULL or the receiver variable's live slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_any_unshift(
    arr: *mut c_void,
    argv: *const u64,
    argc: i64,
    recv_slot: *mut u64,
) -> u64 {
    unsafe {
        // RFC 20260810 刀 D — the relocation walk crosses the
        // unmaterialized tail; loud reject.
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in Array.prototype.unshift\0".as_ptr(),
        ) {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // A new index on a non-extensible receiver is a TypeError
        // (`__torajs_arr_extend_guard`), checked before the first
        // prepend so nothing lands. `argv` is borrowed, so the reject
        // releases nothing — same shape as the sparse reject above.
        if crate::define_length::__torajs_arr_extend_guard(arr) != 0 {
            return __torajs_anyv_box_from_pair(5, 0);
        }
        // A hole or an accessor index: the per-index walk (see
        // `__torajs_arr_unshift_any`). The cell does not move.
        let flags = (*(arr as *const HeapHeader)).flags;
        if flags & FLAG_ARR_ANY != 0 && crate::layout::needs_index_mop(arr as *const u8) {
            let len = __torajs_arr_mutate_generic(arr, torajs_rc::ANY_METHOD_UNSHIFT, argv, argc);
            __torajs_value_drop_heap(len as *mut c_void);
            if __torajs_throw_check() != 0 {
                return ANY_METHOD_THREW;
            }
            return *((arr as *const u8).add(ARR_LEN_OFF) as *const u64);
        }
        let mut cur = arr as *mut u8;
        for i in (0..argc).rev() {
            let av = *argv.add(i as usize);
            let header = &*(cur as *const HeapHeader);
            if header.flags & FLAG_ARR_ANY != 0 {
                cur = any_unshift_one(cur, av);
            } else {
                let Some(raw) = coerce_typed_slot(header.arr_elem_kind(), av) else {
                    return kind_mismatch_threw(
                        c"unshift through an any array receiver would change the array's element kind",
                    );
                };
                cur = crate::transform::__torajs_arr_unshift(cur, raw as i64);
            }
            if !recv_slot.is_null() {
                *recv_slot = cur as u64;
            }
        }
        *(cur.add(ARR_LEN_OFF) as *const u64)
    }
}

/// Prepend one NaN-box value to an Arr&lt;Any&gt; (no deque head by
/// layout contract — reserve + memmove right + store). The stored
/// copy takes its own reference for cells.
unsafe fn any_unshift_one(p: *mut u8, av: u64) -> *mut u8 {
    unsafe {
        // NaN-box-safe inc on the box bits directly (rotation 546
        // form): a cell's box IS the pointer, immediates no-op. The
        // old unbox spelling materialized a ShortStr into an owned
        // Str, inc'd THAT, then adopted the original box — the
        // double-staked materialization leaked whole.
        __torajs_rc_inc(av as *mut c_void);
        any_unshift_adopt(p, av)
    }
}

/// Prepend core — memmove right + store slot 0 + len bump. Adopts
/// `av` as-is (no inc): the caller either owns the stake it hands
/// over (typed-tier adopt contract, mirror of `arr_push_any`) or
/// has already inc'd it (`any_unshift_one` borrow contract).
unsafe fn any_unshift_adopt(p: *mut u8, av: u64) -> *mut u8 {
    unsafe {
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        let cur = crate::grow::__torajs_arr_reserve(p, (len + 1) as i64);
        core::ptr::copy(arr_data(cur), arr_data(cur).add(8), (len as usize) * 8);
        *(arr_data(cur) as *mut u64) = av;
        *(cur.add(ARR_LEN_OFF) as *mut u64) = len + 1;
        cur
    }
}

/// `xs.unshift(v)` for `Array<Any>` on the typed tier — (tag, value)
/// pair form, mirror of `__torajs_arr_push_any`'s adopt contract
/// (the caller transfers ONE refcount with the pair; immediates
/// carry none). Returns the possibly-realloc'd array pointer; the
/// caller stores it back and reads the new length.
///
/// # Safety
/// `arr` is a valid Array<Any> heap block (FLAG_ARR_ANY, 8-byte
/// AnyValue slots).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_unshift_any(arr: *mut u8, tag: i64, value: i64) -> *mut u8 {
    unsafe {
        // RFC 20260810 刀 D — same loud reject as `arr_any_unshift`.
        // This entry ADOPTS the pair's one transferred rc, so every
        // reject releases it (`__torajs_arr_push_any`'s shape); the
        // sparse arm used to return without doing so and leaked the
        // value on each refused call.
        if crate::sparse_gate::sparse_tail_rejects(
            arr as *const c_void,
            b"sparse array tail is not yet supported in Array.prototype.unshift\0".as_ptr(),
        ) {
            crate::any_typed_bridge::drop_pair(tag as u64, value as u64);
            return arr;
        }
        // push's twin — a new index on a non-extensible receiver is a
        // TypeError (`__torajs_arr_extend_guard`).
        if crate::define_length::__torajs_arr_extend_guard(arr as *const c_void) != 0 {
            crate::any_typed_bridge::drop_pair(tag as u64, value as u64);
            return arr;
        }
        // Chunk 628 — a typed block behind a static Arr<Any> view
        // (T-11 container widen) kind-coerces into a raw slot instead
        // of adopting NaN-box bits (622's push twin, the station that
        // pass missed).
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY == 0 {
            return crate::any_typed_bridge::typed_unshift_pair(arr, tag as u64, value as u64);
        }
        let av = __torajs_anyv_box_from_pair(tag, value);
        // A hole or an accessor index moves through the per-index walk
        // (§23.1.3.36 — the raw memmove left a shadow hole where it was).
        // The walk stores its own share; this entry owns the pair's.
        if crate::layout::needs_index_mop(arr) {
            let len =
                __torajs_arr_mutate_generic(arr.cast(), torajs_rc::ANY_METHOD_UNSHIFT, &av, 1);
            __torajs_value_drop_heap(len as *mut c_void);
            __torajs_value_drop_heap(av as *mut c_void);
            return arr;
        }
        any_unshift_adopt(arr, av)
    }
}
