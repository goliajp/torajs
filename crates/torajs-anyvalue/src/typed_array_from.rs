//! §23.2.2.1 %TypedArray%.from / §23.2.2.2 %TypedArray%.of as
//! RUNTIME kernels — the statics a TypedArray constructor VALUE
//! answers on the any lane (`TA.from(src, mapfn?, thisArg?)` where
//! `TA` is a `: any` binding or parameter — the body shape of
//! test262's `testWithTypedArrayConstructors` — and the
//! `%TypedArray%.from.call(C, …)` face). Both statics live on
//! %TypedArray%, not on each concrete constructor, so ONE interned
//! cell per name serves all twelve element kinds
//! (`Uint8Array.from === Int8Array.from`, §23.2.2); the receiver
//! channel carries which constructor the call came off.
//!
//! What differs from `Array.from` (`array_from.rs`) — and why this
//! is not that kernel behind a flag:
//!
//! - an iterable source is drained to a list FIRST (step 5.a
//!   IteratorToList), the target is created with that length, and
//!   only then does the map / set loop run — `mapfn` never sees a
//!   half-built target, and the iterator is exhausted before any
//!   mapped value exists (`Array.from` interleaves per element);
//! - the target is `TypedArrayCreate(C, «len»)` in BOTH branches
//!   (`Array.from`'s iterable branch is a length-less
//!   `Construct(C)`), followed by §23.2.4.2's validation: the product
//!   must be a TypedArray whose length covers `len`, else TypeError;
//! - elements land through `Set(target, k, v, true)` (§10.4.5.5 —
//!   ToNumber / ToBigInt coercion, the wrong-kind TypeError), never
//!   through CreateDataProperty.
//!
//! Ownership: list elements are OWNED until the map call consumes
//! them (`map_elem` releases its input and answers an owned mapped
//! value), and the index-set transfers that stake into the target.
//! The answer is the owned target; a pending throw answers undefined
//! with every still-collected element and the target released.

use torajs_rc::{HeapHeader, Tag};

use crate::array_from::{MapPair, map_elem, mapping_gate};
use crate::iter_any_get_method::{GetIterator, generic_iter_step, get_iterator};
use crate::method_value::arg_at;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_rc_inc};

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
    /// torajs-buffer — §23.2.3.18 `get %TypedArray%.prototype.length`.
    fn __torajs_typedarray_length(recv: AnyValue) -> i64;
}

/// The recv-first dispatch arm: argv[0] is the constructor the call
/// came off (undefined on a bare detached call — step 2's
/// IsConstructor gate refuses it).
///
/// # Safety
/// `argv` points at `argc` live AnyValue slots borrowed for the call.
pub(crate) unsafe fn typed_array_from_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        typed_array_from_this(
            arg_at(argv, argc, 0),
            arg_at(argv, argc, 1),
            arg_at(argv, argc, 2),
            arg_at(argv, argc, 3),
        )
    }
}

/// §23.2.2.2 %TypedArray%.of(...items) — len is the item count,
/// `TypedArrayCreate(C, «len»)`, then one Set per item in order.
///
/// # Safety
/// Same contract as [`typed_array_from_value`].
pub(crate) unsafe fn typed_array_of_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let c = arg_at(argv, argc, 0);
        let n = (argc - 1).max(0);
        if !crate::construct::__torajs_is_constructor(c) {
            __torajs_throw_type_error(c"TypedArray.of: this is not a constructor".as_ptr());
            return VALUE_UNDEFINED;
        }
        let Some(mut target) = typed_array_create(c, n) else {
            return VALUE_UNDEFINED;
        };
        for k in 0..n {
            let v = arg_at(argv, argc, k + 1);
            // argv is borrowed; the set transfers an owned stake.
            __torajs_anyv_rc_inc(v);
            if !set_elem(&mut target, k, v) {
                __torajs_anyv_rc_dec(target);
                return VALUE_UNDEFINED;
            }
        }
        target
    }
}

/// §23.2.2.1 with `this` from the call site. Answers the owned
/// target; a pending throw answers undefined with the throw recorded.
///
/// # Safety
/// `c` / `source` / `mapfn` / `this_arg` are live AnyValues borrowed
/// for the duration of the call.
pub(crate) unsafe fn typed_array_from_this(
    c: AnyValue,
    source: AnyValue,
    mapfn: AnyValue,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        if !crate::construct::__torajs_is_constructor(c) {
            __torajs_throw_type_error(c"TypedArray.from: this is not a constructor".as_ptr());
            return VALUE_UNDEFINED;
        }
        let Ok(map_pair) = mapping_gate(mapfn, c"TypedArray.from: mapfn is not a function") else {
            return VALUE_UNDEFINED;
        };
        // Step 4 — usingIterator: a user `@@iterator` is resolved
        // (and called) ONCE here; a builtin iterable keeps the
        // cascade lane; neither → the array-like walk.
        match get_iterator(source) {
            GetIterator::Threw => VALUE_UNDEFINED,
            GetIterator::Iterator(it) => {
                let values = drain_user_iterator(it);
                __torajs_anyv_rc_dec(it);
                match values {
                    Some(v) => from_list(c, v, map_pair, this_arg),
                    None => VALUE_UNDEFINED,
                }
            }
            GetIterator::NoUserMethod | GetIterator::Absent => {
                if crate::iter_any_claims::claims_iterable(source) {
                    match drain_builtin_iterable(source) {
                        Some(v) => from_list(c, v, map_pair, this_arg),
                        None => VALUE_UNDEFINED,
                    }
                } else {
                    from_array_like(c, source, map_pair, this_arg)
                }
            }
        }
    }
}

/// Step 5.a IteratorToList over a user iterator — every element
/// owned. `None` = a step threw (the collected prefix is released;
/// an abrupt IteratorStep propagates without a close).
unsafe fn drain_user_iterator(it: AnyValue) -> Option<Vec<AnyValue>> {
    unsafe {
        let mut values = Vec::new();
        let mut out: AnyValue = VALUE_UNDEFINED;
        loop {
            let live = generic_iter_step(it, &mut out, crate::iter_any_await::AwaitMode::Sync);
            if __torajs_throw_check() != 0 {
                release_all(values);
                return None;
            }
            if live == 0 {
                return Some(values);
            }
            values.push(out);
        }
    }
}

/// Step 5.a over a builtin iterable (a string / array / Map / Set /
/// iterator cell with no user override) — the plain cascade.
unsafe fn drain_builtin_iterable(source: AnyValue) -> Option<Vec<AnyValue>> {
    unsafe {
        let mut values = Vec::new();
        let mut idx: i64 = 0;
        let mut iter_slot: AnyValue = VALUE_UNDEFINED;
        let mut out: AnyValue = VALUE_UNDEFINED;
        loop {
            let live =
                crate::iter_any::__torajs_any_iter_next(source, &mut idx, &mut iter_slot, &mut out);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(iter_slot);
                release_all(values);
                return None;
            }
            if live == 0 {
                __torajs_anyv_rc_dec(iter_slot);
                return Some(values);
            }
            values.push(out);
        }
    }
}

/// Steps 5.b–5.f — the target sized to the drained list, then one
/// map / Set per element in order.
unsafe fn from_list(
    c: AnyValue,
    values: Vec<AnyValue>,
    map_pair: MapPair,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        let len = values.len() as i64;
        let Some(mut target) = typed_array_create(c, len) else {
            release_all(values);
            return VALUE_UNDEFINED;
        };
        let mut rest = values.into_iter();
        let mut k: i64 = 0;
        while let Some(v) = rest.next() {
            let stored = match map_elem(map_pair, this_arg, v, k) {
                Ok(mapped) => set_elem(&mut target, k, mapped),
                Err(()) => false,
            };
            if !stored {
                release_all(rest.collect());
                __torajs_anyv_rc_dec(target);
                return VALUE_UNDEFINED;
            }
            k += 1;
        }
        target
    }
}

/// Steps 7–12 array-like — LengthOfArrayLike BEFORE the mint, then
/// per-index Get / map / Set (the element read and the map call
/// interleave per index here, unlike the iterable branch).
unsafe fn from_array_like(
    c: AnyValue,
    source: AnyValue,
    map_pair: MapPair,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        let len = crate::iter_any_array_like::array_like_length(source) as i64;
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let Some(mut target) = typed_array_create(c, len) else {
            return VALUE_UNDEFINED;
        };
        for k in 0..len {
            let elem = crate::index_any::__torajs_any_index_get(source, k);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(target);
                return VALUE_UNDEFINED;
            }
            let stored = match map_elem(map_pair, this_arg, elem, k) {
                Ok(mapped) => set_elem(&mut target, k, mapped),
                Err(()) => false,
            };
            if !stored {
                __torajs_anyv_rc_dec(target);
                return VALUE_UNDEFINED;
            }
        }
        target
    }
}

/// §23.2.4.2 TypedArrayCreate(C, «len») — Construct, then the
/// ValidateTypedArray shape check and the step-3 length check. `None`
/// = a TypeError is pending (the product, if any, released).
unsafe fn typed_array_create(c: AnyValue, len: i64) -> Option<AnyValue> {
    unsafe {
        let ctor_argv = [crate::nanbox_encode::__torajs_anyv_box_i64(len)];
        let product = crate::construct::__torajs_anyv_construct(c, ctor_argv.as_ptr(), 1);
        if __torajs_throw_check() != 0 {
            return None;
        }
        let is_view = is_cell(product)
            && (*(as_void_ptr(product) as *const HeapHeader)).type_tag == Tag::TypedArray as u16;
        if !is_view {
            __torajs_anyv_rc_dec(product);
            __torajs_throw_type_error(
                c"TypedArray.from: constructor did not produce a TypedArray".as_ptr(),
            );
            return None;
        }
        if __torajs_typedarray_length(product) < len {
            __torajs_anyv_rc_dec(product);
            __torajs_throw_type_error(
                c"TypedArray.from: constructed TypedArray is too short".as_ptr(),
            );
            return None;
        }
        Some(product)
    }
}

/// `Set(target, k, owned, true)` — the strict any-lane index store
/// (§10.4.5.5 coercion inside; the pair transfers). `false` = the
/// store recorded a throw.
unsafe fn set_elem(target: &mut AnyValue, k: i64, owned: AnyValue) -> bool {
    unsafe {
        let t = crate::__torajs_anyv_unbox_tag(owned);
        let p = crate::__torajs_anyv_unbox_value(owned);
        crate::index_any_set::__torajs_any_index_set(
            *target,
            k,
            t as u64,
            p as u64,
            core::ptr::null_mut::<u64>(),
        );
        __torajs_throw_check() == 0
    }
}

/// Release every owned element of an abandoned list.
unsafe fn release_all(values: Vec<AnyValue>) {
    for v in values {
        unsafe { __torajs_anyv_rc_dec(v) };
    }
}
