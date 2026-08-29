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
//!   pad:6 | inner:8 | next:8 | props:8 }                 (64 B)
//! ```
//!
//! `inner` is the flatMap current-inner-iterator slot (undefined for
//! every other kind; carried in the one layout so the cell never
//! needs a second shape). `next` is the GetIteratorDirect cache —
//! the underlying's next method, read ONCE at mint through
//! [`crate::iter_helper_next::resolve_next_method`] (undefined =
//! the legacy per-step driver; see that module's doc for the lane
//! split and remaining inner-iterator boundary).

use core::ffi::c_void;

// The `{ value, done }` mint lives with the IteratorResult reads;
// re-exported so the method faces keep their import face.
pub(crate) use crate::iter_any_result::iter_result_obj;
use crate::method_call_closure_dispatch::{
    closure_cell_entry, invoke_boxed, invoke_boxed_recv_first, recv_first_shift,
};
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
/// §27.1.6.2 WrapForValidIterator (`Iterator.from` over a
/// non-Iterator) — `fn` slot stays undefined; next/return forward
/// to the underlying (刀 4).
pub(crate) const ITER_HELPER_WRAP: u8 = 4;
/// §27.1.4.5 flatMap — the `inner` slot drives (刀 4).
pub(crate) const ITER_HELPER_FLAT_MAP: u8 = 5;
/// `Iterator.concat` (proposal-iterator-sequencing, 刀 5a) — the
/// underlying slot holds the items `Array<Any>`, counter is the
/// next-item index, and the inner slot drives like flatMap's.
pub(crate) const ITER_HELPER_CONCAT: u8 = 6;
/// `Iterator.zip` (proposal-joint-iteration, 刀 5b) — underlying is
/// the open-iterators `Array<Any>`, fn is the longest-mode padding
/// list, counter is the mode.
pub(crate) const ITER_HELPER_ZIP: u8 = 7;
/// `Iterator.zipKeyed` (刀 5c) — zip's shape plus the keys snapshot
/// (`Array<Str>`, HEAP-marked) in the inner slot; rows are objects.
pub(crate) const ITER_HELPER_ZIP_KEYED: u8 = 8;

pub(crate) const UNDERLYING_OFF: usize = 8;
pub(crate) const FN_OFF: usize = 16;
pub(crate) const COUNTER_OFF: usize = 24;
const KIND_OFF: usize = 32;
pub(crate) const ALIVE_OFF: usize = 33;
/// §27.5.3.2 "executing" state byte (third flag in the pad;
/// `alloc_zeroed` starts it 0 = not running).
const RUNNING_OFF: usize = 34;
/// The flatMap current-inner-iterator slot.
pub(crate) const INNER_OFF: usize = 40;
/// The GetIteratorDirect next-method cache.
pub(crate) const NEXT_OFF: usize = 48;
/// Lazy own-property bag — §27.1.4.x mints an ORDINARY object, so
/// `h.zz = 1` is an ordinary own property while the underlying /
/// callback / counter above are internal state. NULL until the first
/// such write (`alloc_zeroed` starts it there). Mirrored by
/// `member_get_layout::ITER_HELPER_PROPS_OFF`.
pub(crate) const PROPS_OFF: usize = 56;
const CELL_SIZE: usize = 64;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

// Validation faces — the IfAbruptCloseIterator close + the tag-15
// ownership predicate (`validation.rs` sibling, file-size cap).
mod validation;
use validation::close_on_validation_abrupt;
pub(crate) use validation::iter_proto_owns_mid;

/// Mint a helper cell over `recv` (§27.1.4.x steps 1-4). Non-object
/// receivers and non-callable callbacks take the spec TypeError and
/// answer undefined (the pending throw propagates through the
/// dispatch face's throw check).
///
/// # Safety
/// `recv` / `fn_av` carry valid AnyValue bit patterns.
pub(crate) unsafe fn iter_helper_mint(recv: AnyValue, kind: u8, fn_av: AnyValue) -> AnyValue {
    // §27.1.4.x step 1 — "is an Object", not "is a heap cell": a
    // primitive Str / Symbol / BigInt cell is still a primitive and
    // takes the TypeError (rotation 434 — the `.call(0)` reflection
    // family plants poisoned wrapper-prototype `next` getters to
    // catch a ToObject here).
    if !unsafe { crate::iter_zip_shared::av_is_object(recv) } {
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
    // bits. A validation abrupt is IfAbruptCloseIterator (the
    // Iterator Record exists from step 2, its next method not yet
    // read): close the underlying FIRST, then land the error —
    // the receiver-not-an-object TypeError above stays close-free
    // (it precedes the record).
    let fn_slot: u64 = if numeric_kind {
        let (t, p) = (
            crate::__torajs_anyv_unbox_tag(fn_av),
            crate::__torajs_anyv_unbox_value(fn_av),
        );
        let n = unsafe { crate::coerce::any_to_number(t, p) };
        if unsafe { __torajs_throw_check() } != 0 {
            // ToNumber itself threw (a valueOf poison) — close and
            // let the poison's throw win (§7.4.9 step 5).
            unsafe { close_on_validation_abrupt(recv) };
            return VALUE_UNDEFINED;
        }
        if n.is_nan() || n < 0.0 {
            unsafe {
                close_on_validation_abrupt(recv);
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
                close_on_validation_abrupt(recv);
                __torajs_throw_type_error(c"Iterator helper callback is not a function".as_ptr());
            }
            return VALUE_UNDEFINED;
        }
        fn_av
    };
    // §27.1.4.x GetIteratorDirect — cache the next method (a Get
    // that may fire an accessor exactly once; a throw forwards).
    let Ok(next_slot) = (unsafe { crate::iter_helper_next::resolve_next_method(recv) }) else {
        return VALUE_UNDEFINED;
    };
    unsafe {
        if !numeric_kind {
            torajs_rc::__torajs_rc_inc(as_void_ptr(fn_slot) as *mut c_void);
        }
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
        *(cell.add(INNER_OFF) as *mut u64) = VALUE_UNDEFINED;
        *(cell.add(NEXT_OFF) as *mut u64) = next_slot;
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// Fresh rc-1 helper cell of `kind` with every AnyValue slot set to
/// undefined — the shared alloc under [`iter_helper_mint_wrap`] and
/// the concat mint (each fills its own underlying slot after).
pub(crate) unsafe fn iter_helper_cell_alloc(kind: u8) -> *mut u8 {
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::IterHelper as u16;
        *(cell.add(UNDERLYING_OFF) as *mut u64) = VALUE_UNDEFINED;
        *(cell.add(FN_OFF) as *mut u64) = VALUE_UNDEFINED;
        *(cell.add(KIND_OFF) as *mut u8) = kind;
        *(cell.add(ALIVE_OFF) as *mut u8) = 1;
        *(cell.add(INNER_OFF) as *mut u64) = VALUE_UNDEFINED;
        *(cell.add(NEXT_OFF) as *mut u64) = VALUE_UNDEFINED;
        cell
    }
}

/// Mint a kind-WRAP cell over an OWNED underlying — §27.1.6.2
/// WrapForValidIterator. The caller's reference TRANSFERS to the
/// cell's underlying slot (unlike [`iter_helper_mint`], whose
/// receiver is a dispatcher borrow); the fn slot stays undefined.
///
/// # Safety
/// `underlying` is an owned live cell AnyValue.
pub(crate) unsafe fn iter_helper_mint_wrap(underlying: AnyValue) -> AnyValue {
    unsafe {
        // §27.1.6.2 GetIteratorDirect — the wrapped record carries
        // the cached next; a throwing Get releases and forwards.
        let Ok(next_slot) = crate::iter_helper_next::resolve_next_method(underlying) else {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(underlying);
            return VALUE_UNDEFINED;
        };
        let cell = iter_helper_cell_alloc(ITER_HELPER_WRAP);
        *(cell.add(UNDERLYING_OFF) as *mut u64) = underlying;
        *(cell.add(NEXT_OFF) as *mut u64) = next_slot;
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
        // §27.5.3.2 GeneratorValidate step 6 — a helper is a spec
        // generator, and a re-entrant resume (a user next() called
        // from inside the callback / inner iterator this step is
        // driving) throws a catchable TypeError. Without the gate
        // the re-entry re-drove the same inner and recursed to a
        // stack overflow (test262 Iterator/concat
        // throws-typeerror-when-generator-is-running-next, exit 139).
        if (ptr.cast::<u8>().add(RUNNING_OFF)).read() != 0 {
            __torajs_throw_type_error(c"Iterator Helper is already running".as_ptr());
            return 0;
        }
        if (ptr.cast::<u8>().add(ALIVE_OFF)).read() == 0 {
            return 0;
        }
        (ptr.cast::<u8>().add(RUNNING_OFF)).write(1);
        let r = iter_helper_step_inner(ptr, out);
        (ptr.cast::<u8>().add(RUNNING_OFF)).write(0);
        r
    }
}

/// The pre-guard step body — every arm of the kind dispatch, exactly
/// as it ran before the executing gate wrapped it.
unsafe fn iter_helper_step_inner(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let underlying = (ptr.cast::<u8>().add(UNDERLYING_OFF) as *const u64).read();
        let kind = (ptr.cast::<u8>().add(KIND_OFF)).read();
        // §27.1.4.5 flatMap — the double loop (inner drain + outer
        // step + flatten) lives with GetIteratorFlattenable.
        if kind == ITER_HELPER_FLAT_MAP {
            return crate::iter_from::iter_flat_map_step(ptr, out);
        }
        // Iterator.concat — the sequenced-items double loop (刀 5a).
        if kind == ITER_HELPER_CONCAT {
            return crate::iter_concat::iter_concat_step(ptr, out);
        }
        // Iterator.zip / zipKeyed — the joint-iteration row step
        // (刀 5b/5c; the flag picks the row shape).
        if kind == ITER_HELPER_ZIP || kind == ITER_HELPER_ZIP_KEYED {
            return crate::iter_zip::iter_zip_step(ptr, out, kind == ITER_HELPER_ZIP_KEYED);
        }
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
                let hit = crate::iter_helper_next::helper_underlying_step(ptr, &mut skipped);
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
            let hit = crate::iter_helper_next::helper_underlying_step(ptr, &mut item);
            if hit == 0 {
                // Underlying done or threw — either way this helper
                // is finished (§27.1.4.2 step 5.b.ii; a throw
                // propagates through the caller's throw check).
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                return 0;
            }
            let counter = (ptr.cast::<u8>().add(COUNTER_OFF) as *const u64).read();
            (ptr.cast::<u8>().add(COUNTER_OFF) as *mut u64).write(counter + 1);
            // take / drop pass the item through untouched; WRAP is
            // the §27.1.6.3.1 next() forward — same pass-through.
            if kind == ITER_HELPER_TAKE || kind == ITER_HELPER_DROP || kind == ITER_HELPER_WRAP {
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
            // §27.1.4.x 5.b — Call(mapper/predicate, undefined, …):
            // same receiver-first seeding as the eager consumers.
            let result = if recv_first_shift(env) != 0 {
                invoke_boxed_recv_first(env, entry, VALUE_UNDEFINED, argv.as_ptr(), 2)
            } else {
                invoke_boxed(env, entry, argv.as_ptr(), 2)
            };
            if __torajs_throw_check() != 0 {
                // §27.1.4.6 step 5.b.v — callback threw: close the
                // underlying (under the stashed throw, so its
                // return() actually runs), kill the helper, forward
                // the throw.
                crate::nanbox_ffi::__torajs_anyv_rc_dec(item);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(result);
                (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
                crate::iter_any_close::iter_close_under_pending_throw(underlying);
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
                // §27.5.3.4 GeneratorResumeAbrupt validates the same
                // "executing" state — a return() from inside a
                // running step is a catchable TypeError, not a close.
                // But return() itself does NOT hold the flag: spec
                // §27.1.4.x return sets the state to completed FIRST
                // and then runs IteratorCloseAll, so a next()/
                // return() re-entered from a closing iterator's own
                // return() observes "completed" and answers done
                // (test262 zip suspended-start-iterator-close-calls-
                // next — the executing gate must not fire there).
                if (ptr.cast::<u8>().add(RUNNING_OFF)).read() != 0 {
                    __torajs_throw_type_error(c"Iterator Helper is already running".as_ptr());
                    return VALUE_UNDEFINED;
                }
                iter_helper_do_return(ptr)
            }
            _ => try_helper_chain(ptr, mid, argv, argc)
                .unwrap_or_else(|| crate::method_call::method_no_such()),
        }
    }
}

/// §27.1.5.2 — close the underlying, answer `{ value: undefined,
/// done: true }`. A WRAP cell instead FORWARDS return() and passes
/// the underlying's own result through (§27.1.6.3.2). Runs under the
/// caller's executing gate (the close can re-enter user code).
unsafe fn iter_helper_do_return(ptr: *mut c_void) -> AnyValue {
    unsafe {
        if (ptr.cast::<u8>().add(ALIVE_OFF)).read() != 0 {
            (ptr.cast::<u8>().add(ALIVE_OFF)).write(0);
            let underlying = (ptr.cast::<u8>().add(UNDERLYING_OFF) as *const u64).read();
            if (ptr.cast::<u8>().add(KIND_OFF)).read() == ITER_HELPER_WRAP {
                return crate::iter_from::wrap_return_forward(underlying);
            }
            // CONCAT's underlying is the items list, not an
            // iterator — only the open inner needs closing.
            if (ptr.cast::<u8>().add(KIND_OFF)).read() == ITER_HELPER_CONCAT {
                crate::iter_concat::iter_concat_close_inner(ptr);
                return iter_result_obj(VALUE_UNDEFINED, true);
            }
            // ZIP / ZIP_KEYED's underlying is the open-columns
            // list — close every still-open column.
            if matches!(
                (ptr.cast::<u8>().add(KIND_OFF)).read(),
                ITER_HELPER_ZIP | ITER_HELPER_ZIP_KEYED
            ) {
                crate::iter_zip::iter_zip_close_all(ptr);
                return iter_result_obj(VALUE_UNDEFINED, true);
            }
            crate::iter_any_close::__torajs_iter_close_value(underlying);
        }
        iter_result_obj(VALUE_UNDEFINED, true)
    }
}

mod chain;
pub(crate) use chain::try_helper_chain;

/// Drop glue — the torajs-value-drop dispatcher contract is
/// RELEASE ONE REFERENCE: rc-dec the cell itself, and only on
/// hit-zero release the four owned AnyValue slots and free the
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
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(INNER_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(NEXT_OFF) as *const u64).read());
        // Own-property bag — a raw cell pointer, not an AnyValue, so
        // it takes the universal dispatcher rather than the nan-box
        // release above.
        let props = (p.add(PROPS_OFF) as *const u64).read() as *mut c_void;
        if !props.is_null() {
            (p.add(PROPS_OFF) as *mut u64).write(0);
            crate::__torajs_value_drop_heap(props);
        }
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(p, layout);
    }
}
