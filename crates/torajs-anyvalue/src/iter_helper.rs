//! Iterator Helper cells (`Tag::IterHelper` = 25) — RFC
//! 20260730-iterator-global 刀 2.
//!
//! §27.1.4.x lazy helpers (`.map(fn)` first; the family grows per
//! blade). A helper cell owns its underlying iterator (any AnyValue
//! honoring the iterator protocol — generator instances, iterator
//! cells, helper cells for chaining) plus the captured callback, a
//! spec counter, and an alive flag:
//!
//! ```text
//! { header:8 | underlying:8 | fn:8 | counter:8 | kind:1 alive:1
//!   pad:6 | inner:8 }                                    (48 B)
//! ```
//!
//! `inner` is the flatMap current-inner-iterator slot (undefined
//! until that kind lands; carried in the layout so the cell never
//! needs a second shape).
//!
//! Recorded boundary (RFC §3.3 deviation): the spec's
//! GetIteratorDirect caches `next` at helper-construction time; this
//! implementation re-reads it per step through
//! [`step_derived_iterator`] (the §7.4.2-6 driver every other
//! consumer uses). The get-next-method-only-once test262 shape
//! diverges; everything else agrees.

use core::ffi::c_void;

use crate::iter_helper_eager::{iter_eager, iter_to_array};
use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_boxed};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use crate::{as_void_ptr, is_cell};
use torajs_rc::Tag;

/// Helper kinds (the `kind` byte).
pub(crate) const ITER_HELPER_MAP: u8 = 0;
/// §27.1.4.6-sibling predicate helper (刀 2b).
pub(crate) const ITER_HELPER_FILTER: u8 = 1;
/// §27.1.4.9 — `fn` slot holds the remaining-count f64 bits.
pub(crate) const ITER_HELPER_TAKE: u8 = 2;
/// §27.1.4.3 — `fn` slot holds the still-to-skip f64 bits.
pub(crate) const ITER_HELPER_DROP: u8 = 3;

const UNDERLYING_OFF: usize = 8;
const FN_OFF: usize = 16;
const COUNTER_OFF: usize = 24;
const KIND_OFF: usize = 32;
const ALIVE_OFF: usize = 33;
const CELL_SIZE: usize = 48;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
}

/// Mint a helper cell over `recv` (§27.1.4.x steps 1-4). Non-object
/// receivers and non-callable callbacks take the spec TypeError and
/// answer undefined (the pending throw propagates through the
/// dispatch face's throw check).
///
/// # Safety
/// `recv` / `fn_av` carry valid AnyValue bit patterns.
pub(crate) unsafe fn iter_helper_mint(recv: AnyValue, kind: u8, fn_av: AnyValue) -> AnyValue {
    if !is_cell(recv) {
        unsafe {
            __torajs_throw_type_error(c"Iterator helper called on a non-object".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    let numeric_kind = kind == ITER_HELPER_TAKE || kind == ITER_HELPER_DROP;
    // Callable / limit validation BEFORE touching the underlying
    // (§27.1.4.x step 2 precedes GetIteratorDirect). take/drop run
    // §27.1.4.9/.3 steps 3-6: ToNumber, NaN or negative →
    // RangeError; the (possibly +∞) count rides the fn slot as f64
    // bits.
    let fn_slot: u64 = if numeric_kind {
        let (t, p) = (
            crate::__torajs_anyv_unbox_tag(fn_av),
            crate::__torajs_anyv_unbox_value(fn_av),
        );
        let n = unsafe { crate::coerce::any_to_number(t, p) };
        if n.is_nan() || n < 0.0 {
            unsafe {
                __torajs_throw_range_error(c"Iterator helper limit must be non-negative".as_ptr());
            }
            return VALUE_UNDEFINED;
        }
        n.trunc().to_bits()
    } else {
        if !is_cell(fn_av)
            || unsafe { closure_cell_entry(as_void_ptr(fn_av) as *mut c_void) }.is_none()
        {
            unsafe {
                __torajs_throw_type_error(c"Iterator helper callback is not a function".as_ptr());
            }
            return VALUE_UNDEFINED;
        }
        unsafe { torajs_rc::__torajs_rc_inc(as_void_ptr(fn_av) as *mut c_void) };
        fn_av
    };
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::IterHelper as u16;
        // flags stay 0 (multi-thread-ready shape: no single-mutator
        // assumption baked in; the header is the universal one).
        torajs_rc::__torajs_rc_inc(as_void_ptr(recv) as *mut c_void);
        *(cell.add(UNDERLYING_OFF) as *mut u64) = recv;
        // fn-kinds stored their +1 above; numeric kinds store plain
        // f64 bits (never cell-shaped — the drop glue's anyv_rc_dec
        // no-ops on non-cell patterns).
        *(cell.add(FN_OFF) as *mut u64) = fn_slot;
        *(cell.add(KIND_OFF) as *mut u8) = kind;
        *(cell.add(ALIVE_OFF) as *mut u8) = 1;
        *(cell.add(40) as *mut u64) = VALUE_UNDEFINED;
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// One protocol step of the helper itself — the shared core the
/// method face (`next()` → IteratorResult dynobj) and the for-of
/// any-lane both drive. Returns 1 with `*out` owned on a value, 0 on
/// done (or on a pending throw — the caller's throw check tells the
/// two apart).
///
/// # Safety
/// `ptr` is a live IterHelper cell; `out` is writable.
pub(crate) unsafe fn iter_helper_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        *out = VALUE_UNDEFINED;
        if (ptr.cast::<u8>().add(ALIVE_OFF)).read() == 0 {
            return 0;
        }
        let underlying = (ptr.cast::<u8>().add(UNDERLYING_OFF) as *const u64).read();
        let kind = (ptr.cast::<u8>().add(KIND_OFF)).read();
        // §27.1.4.9 take — a zero remaining-count is done BEFORE the
        // underlying steps (and closes it, step 5.a.ii).
        if kind == ITER_HELPER_TAKE {
            let remaining = f64::from_bits((ptr.cast::<u8>().add(FN_OFF) as *const u64).read());
            if remaining <= 0.0 {
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                crate::iter_any_close::__torajs_iter_close_value(underlying);
                return 0;
            }
            (ptr.cast::<u8>().add(FN_OFF) as *mut u64).write((remaining - 1.0).to_bits());
        }
        // §27.1.4.3 drop — the skip runs once, ahead of the first
        // real step (counter == 0 marks "not started").
        if kind == ITER_HELPER_DROP && (ptr.cast::<u8>().add(COUNTER_OFF) as *const u64).read() == 0
        {
            let mut to_skip = f64::from_bits((ptr.cast::<u8>().add(FN_OFF) as *const u64).read());
            while to_skip > 0.0 {
                let mut skipped: AnyValue = VALUE_UNDEFINED;
                let hit =
                    crate::iter_any_step::step_derived_iterator(underlying, &mut skipped, false);
                if hit == 0 {
                    (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                    return 0;
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(skipped);
                to_skip -= 1.0;
            }
        }
        loop {
            let mut item: AnyValue = VALUE_UNDEFINED;
            let hit = crate::iter_any_step::step_derived_iterator(underlying, &mut item, false);
            if hit == 0 {
                // Underlying done or threw — either way this helper
                // is finished (§27.1.4.2 step 5.b.ii; a throw
                // propagates through the caller's throw check).
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                return 0;
            }
            let counter = (ptr.cast::<u8>().add(COUNTER_OFF) as *const u64).read();
            (ptr.cast::<u8>().add(COUNTER_OFF) as *mut u64).write(counter + 1);
            if kind == ITER_HELPER_TAKE || kind == ITER_HELPER_DROP {
                *out = item;
                return 1;
            }
            let fn_av = (ptr.cast::<u8>().add(FN_OFF) as *const u64).read();
            let Some((env, entry)) = closure_cell_entry(as_void_ptr(fn_av) as *mut c_void) else {
                // Unreachable by construction (mint validated) —
                // treat as done rather than UB.
                crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                return 0;
            };
            // 𝔽(counter) rides the i64 lane (numerically identical).
            let counter_av = crate::__torajs_anyv_box_from_pair(2, counter as i64) as u64;
            let argv = [item, counter_av];
            let result = invoke_boxed(env, entry, argv.as_ptr(), 2);
            if __torajs_throw_check() != 0 {
                // §27.1.4.6 step 5.b.v — callback threw: close the
                // underlying, kill the helper, forward the throw.
                crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(result);
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                crate::iter_any_close::__torajs_iter_close_value(underlying);
                return 0;
            }
            if kind == ITER_HELPER_FILTER {
                // §27.1.4.4 step 5.b.v — ToBoolean(selected): keep
                // the ITEM on truthy, loop on falsy.
                let keep = crate::nanbox_ffi::__torajs_anyv_to_bool(result);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(result);
                if keep {
                    *out = item;
                    return 1;
                }
                crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                continue;
            }
            // MAP — the mapped value replaces the item.
            crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
            *out = result;
            return 1;
        }
    }
}

/// The `Tag::IterHelper` method-dispatch arm: `next` / `return` /
/// chained lazy helpers. Everything else falls to no-such.
///
/// # Safety
/// `ptr` is a live IterHelper cell; `argv` holds `argc` boxed values.
pub(crate) unsafe fn iter_helper_method(
    ptr: *mut c_void,
    mid: i64,
    argv: *const AnyValue,
    argc: i64,
) -> AnyValue {
    unsafe {
        match mid {
            torajs_rc::ANY_METHOD_NEXT => {
                let mut v: AnyValue = VALUE_UNDEFINED;
                let hit = iter_helper_step(ptr, &mut v);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                iter_result_obj(v, hit == 0)
            }
            torajs_rc::any_method::ANY_METHOD_ITER_RETURN => {
                // §27.1.5.2 — close the underlying, answer
                // { value: undefined, done: true }.
                if (ptr.cast::<u8>().add(ALIVE_OFF)).read() != 0 {
                    (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                    let underlying = (ptr.cast::<u8>().add(UNDERLYING_OFF) as *const u64).read();
                    crate::iter_any_close::__torajs_iter_close_value(underlying);
                }
                iter_result_obj(VALUE_UNDEFINED, true)
            }
            _ => try_helper_chain(ptr, mid, argv, argc)
                .unwrap_or_else(|| crate::method_call::method_no_such()),
        }
    }
}

/// Chained lazy-helper construction on any iterator-protocol cell
/// (`m.map(f).map(g)`, `[].values().map(f)`, …) — shared by the
/// IterHelper / MapIter / ArrIter dispatch arms. `None` = not a
/// helper mid (caller falls through to its own face).
///
/// # Safety
/// `ptr` is a live heap cell of the caller's tag; `argv` holds
/// `argc` boxed values.
pub(crate) unsafe fn try_helper_chain(
    ptr: *mut c_void,
    mid: i64,
    argv: *const AnyValue,
    argc: i64,
) -> Option<AnyValue> {
    let kind = match mid {
        torajs_rc::ANY_METHOD_MAP => ITER_HELPER_MAP,
        torajs_rc::ANY_METHOD_FILTER => ITER_HELPER_FILTER,
        torajs_rc::any_method_iter::ANY_METHOD_TAKE => ITER_HELPER_TAKE,
        torajs_rc::any_method_iter::ANY_METHOD_DROP => ITER_HELPER_DROP,
        torajs_rc::any_method_iter::ANY_METHOD_TO_ARRAY => {
            return Some(unsafe { iter_to_array(ptr) });
        }
        // 刀 3 — eager consumers (§27.1.4.5/.7/.8/.2/.12): drive to
        // exhaustion / short-circuit, closing the underlying on an
        // early exit.
        torajs_rc::ANY_METHOD_FOR_EACH
        | torajs_rc::ANY_METHOD_SOME
        | torajs_rc::ANY_METHOD_EVERY
        | torajs_rc::ANY_METHOD_FIND
        | torajs_rc::ANY_METHOD_REDUCE => {
            return Some(unsafe { iter_eager(ptr, mid, argv, argc) });
        }
        _ => return None,
    };
    let fn_av = if argc >= 1 {
        unsafe { argv.read() }
    } else {
        VALUE_UNDEFINED
    };
    // The receiver box is a borrow of the dispatcher's cell; mint
    // takes its own stake.
    Some(unsafe { iter_helper_mint(ptr as u64, kind, fn_av) })
}

/// Fresh `{ value, done }` IteratorResult dynobj; `value` transfers
/// in owned (same ledger as the MapIter/ArrIter next() arms).
unsafe fn iter_result_obj(value: AnyValue, done: bool) -> AnyValue {
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        let (tag, payload) = (
            crate::__torajs_anyv_unbox_tag(value),
            crate::__torajs_anyv_unbox_value(value),
        );
        let k_value = __torajs_str_alloc(c"value".as_ptr() as *const u8, 5);
        __torajs_dynobj_set(&mut obj, k_value as *mut c_void, tag as u64, payload as u64);
        __torajs_str_drop(k_value as *mut c_void);
        let k_done = __torajs_str_alloc(c"done".as_ptr() as *const u8, 4);
        __torajs_dynobj_set(&mut obj, k_done as *mut c_void, 1, done as u64);
        __torajs_str_drop(k_done as *mut c_void);
        obj as u64
    }
}

/// Drop glue — the torajs-value-drop dispatcher contract is
/// RELEASE ONE REFERENCE: rc-dec the cell itself, and only on
/// hit-zero release the three owned AnyValue slots and free the
/// block (mirror of `__torajs_arr_iter_drop`). The 刀 2b churn
/// caught the unconditional-free first cut: a chained helper's
/// underlying release freed the inner cell while its binding still
/// held a reference (double-free SIGTRAP within hundreds of
/// iterations; single-layer cells survived by accident — their one
/// release WAS the last).
///
/// # Safety
/// `cell` is a live IterHelper cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iter_helper_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            return;
        }
        let p = cell.cast::<u8>();
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(UNDERLYING_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(FN_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(40) as *const u64).read());
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(p, layout);
    }
}
