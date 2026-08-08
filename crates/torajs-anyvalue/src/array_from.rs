//! §23.1.2.1 Array.from as a RUNTIME kernel (RFC
//! 20260808-construct-channel B6) — the any-tier walk a detached
//! `const f = Array.from; f(items, mapFn?, thisArg?)` call runs, the
//! `Array.from.call(C, …)` construct face (knife 2: `this` rides the
//! ns-static cell's receiver channel), and the single source the
//! typed lowering's escape shapes route to.
//!
//! The typed tier keeps its fast arms (string / `Array<T>` / Set
//! materialize without boxing); this kernel exists for the shapes
//! those arms cannot be spec-true on:
//!
//! - the source is erased (`any`) or array-like (`{length}` object)
//!   — elements come from the unified iteration cascade
//!   ([`crate::iter_any`]'s `Array.from` entry, whose tail walks
//!   `length` + index keys instead of throwing);
//! - a mapFn is observing its call shape — §23.1.2.1 always calls
//!   `mapfn` with EXACTLY «kValue, k» and binds `thisArg`, so
//!   `arguments.length` is 2 whatever the declared arity;
//! - the element Get and the mapfn call interleave per index
//!   (steps 3.e / 5.e) — an element updated after the walk started
//!   is read at its turn, not snapshotted up front;
//! - `this` is a constructor — step 4 splits on usingIterator:
//!   `Construct(C)` (iterable) vs `Construct(C, «len»)` (array-like),
//!   elements land through CreateDataPropertyOrThrow (the species
//!   walk's define-semantics store), and step 6/3.f writes `length`.
//!
//! Ownership: the walk's out-slot hands OWNED elements; a mapfn call
//! consumes the element (released here after the call, the callee
//! borrows argv) and answers an owned mapped value; storing transfers
//! that stake into the product. The answer is an owned AnyValue — a
//! fresh rc-1 `Array<Any>`, or the construct product.

use core::ffi::c_void;

use crate::iter_any_get_method::{
    GetIterator, generic_iter_close, generic_iter_step, get_iterator,
};
use crate::method_call_arr_species::{store_elem, write_product_length};
use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_int32, box_void_ptr, is_undefined};

unsafe extern "C" {
    /// torajs-arr — fresh rc-1 `Array<Any>` (cap hint only).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// torajs-arr — push an owned (tag, payload) pair; answers the
    /// (possibly relocated) array.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
}

/// The mapfn face after the step-2 gate: `None` = no mapping,
/// `Some((env, entry))` = the boxed callable pair.
type MapPair = Option<(*mut c_void, u64)>;

/// §23.1.2.1 Array.from(items, mapfn?, thisArg?) with `this` from
/// the call site (undefined on a bare detached call; the `.call(C)`
/// receiver channel hands a real one). Step 8/12's IsConstructor
/// verdict picks ArrayCreate vs Construct(C). Answers an owned
/// AnyValue; a pending throw answers undefined with the throw
/// recorded.
///
/// # Safety
/// `this_c` / `items` / `mapfn` / `this_arg` are live AnyValues
/// borrowed for the duration of the call.
pub(crate) unsafe fn array_from_this(
    this_c: AnyValue,
    items: AnyValue,
    mapfn: AnyValue,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        // Step 2 — mapping + IsCallable BEFORE any iteration.
        let map_pair: MapPair = if is_undefined(mapfn) {
            None
        } else {
            match closure_boxed_entry(mapfn) {
                Some(pair) => Some(pair),
                None => {
                    __torajs_throw_type_error(c"Array.from: mapfn is not a function".as_ptr());
                    return VALUE_UNDEFINED;
                }
            }
        };
        if !crate::construct::__torajs_is_constructor(this_c) {
            return from_plain(items, map_pair, this_arg);
        }
        // Step 4 — usingIterator. A user `@@iterator` is resolved
        // (and called) ONCE here; a builtin iterable keeps the
        // cascade lane; neither → the array-like walk.
        match get_iterator(items) {
            GetIterator::Threw => VALUE_UNDEFINED,
            GetIterator::Iterator(it) => {
                let product = from_user_iterator(this_c, it, map_pair, this_arg);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(it);
                product
            }
            GetIterator::NoUserMethod => {
                if crate::iter_any::claims_iterable(items) {
                    from_builtin_iterable(this_c, items, map_pair, this_arg)
                } else {
                    from_array_like(this_c, items, map_pair, this_arg)
                }
            }
        }
    }
}

/// Steps 3.e.iii / 5.e.iii — Call(mapfn, T, «kValue, k»): exactly two
/// args whatever the declared arity (the boxed lane materializes
/// `arguments` off the real argc), thisArg on the receiver channel.
/// Consumes `owned` (the callee borrows argv; released here). `Err`
/// = the mapfn threw (pending recorded).
unsafe fn map_elem(
    map_pair: MapPair,
    this_arg: AnyValue,
    owned: AnyValue,
    k: i64,
) -> Result<AnyValue, ()> {
    unsafe {
        let Some((env, entry)) = map_pair else {
            return Ok(owned);
        };
        let call_argv = [owned, box_int32(k as i32) as u64];
        let mapped = invoke_with_this(env, entry, this_arg, call_argv.as_ptr(), 2);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(owned);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(mapped);
            return Err(());
        }
        Ok(mapped)
    }
}

/// The no-constructor walk (steps 8/12 ArrayCreate): the unified
/// iterable/array-like cascade into a dense `Array<Any>` — the
/// recursive-push store IS CreateDataPropertyOrThrow on a fresh
/// dense array.
unsafe fn from_plain(items: AnyValue, map_pair: MapPair, this_arg: AnyValue) -> AnyValue {
    unsafe {
        let mut arr = __torajs_arr_alloc_any(0);
        let mut idx: i64 = 0;
        let mut iter_slot: AnyValue = VALUE_UNDEFINED;
        let mut out: AnyValue = VALUE_UNDEFINED;
        let mut k: i64 = 0;
        loop {
            let live = crate::iter_any::__torajs_any_iter_next_array_like(
                items,
                &mut idx,
                &mut iter_slot,
                &mut out,
            );
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(arr as *mut c_void));
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return VALUE_UNDEFINED;
            }
            if live == 0 {
                break;
            }
            let Ok(v) = map_elem(map_pair, this_arg, out, k) else {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(arr as *mut c_void));
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return VALUE_UNDEFINED;
            };
            let t = crate::__torajs_anyv_unbox_tag(v);
            let p = crate::__torajs_anyv_unbox_value(v);
            arr = __torajs_arr_push_any(arr as *mut c_void, t as u64, p as u64);
            k += 1;
        }
        // The walk's iterator reference is the caller's to release
        // (an array-like lane parks an immediate there — no-op).
        crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
        box_void_ptr(arr as *mut c_void)
    }
}

/// Steps 5-6 over a user `@@iterator` — A = Construct(C), one
/// IteratorStep per element, define-semantics store, then `length`.
/// An abrupt map/store closes the iterator (IfAbruptCloseIterator).
unsafe fn from_user_iterator(
    c: AnyValue,
    it: AnyValue,
    map_pair: MapPair,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        let mut product = crate::construct::__torajs_anyv_construct(c, core::ptr::null(), 0);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let mut out: AnyValue = VALUE_UNDEFINED;
        let mut k: i64 = 0;
        loop {
            let live = generic_iter_step(it, &mut out, false);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                return VALUE_UNDEFINED;
            }
            if live == 0 {
                break;
            }
            let stored = match map_elem(map_pair, this_arg, out, k) {
                Ok(v) => store_elem(&mut product, k, v),
                Err(()) => false,
            };
            if !stored {
                generic_iter_close(it);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                return VALUE_UNDEFINED;
            }
            k += 1;
        }
        write_product_length(&mut product, k);
        product
    }
}

/// The builtin-iterable side of step 4 (a string / array / Map / Set
/// / iterator cell / `[Symbol.iterator]()` class instance with no
/// user override) — same Construct(C) + define walk, elements from
/// the plain cascade.
unsafe fn from_builtin_iterable(
    c: AnyValue,
    items: AnyValue,
    map_pair: MapPair,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        let mut product = crate::construct::__torajs_anyv_construct(c, core::ptr::null(), 0);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let mut idx: i64 = 0;
        let mut iter_slot: AnyValue = VALUE_UNDEFINED;
        let mut out: AnyValue = VALUE_UNDEFINED;
        let mut k: i64 = 0;
        loop {
            let live =
                crate::iter_any::__torajs_any_iter_next(items, &mut idx, &mut iter_slot, &mut out);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return VALUE_UNDEFINED;
            }
            if live == 0 {
                break;
            }
            let stored = match map_elem(map_pair, this_arg, out, k) {
                Ok(v) => store_elem(&mut product, k, v),
                Err(()) => false,
            };
            if !stored {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return VALUE_UNDEFINED;
            }
            k += 1;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
        write_product_length(&mut product, k);
        product
    }
}

/// Step 3 array-like — len BEFORE the mint (`Construct(C, «len»)`),
/// then per-index Get / map / define, then `length` (step 3.f writes
/// len, not k — they are equal here, the walk visits every index).
unsafe fn from_array_like(
    c: AnyValue,
    items: AnyValue,
    map_pair: MapPair,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        let len = crate::iter_any_array_like::array_like_length(items) as i64;
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let len_box = crate::nanbox_encode::__torajs_anyv_box_i64(len);
        let ctor_argv = [len_box];
        let mut product = crate::construct::__torajs_anyv_construct(c, ctor_argv.as_ptr(), 1);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        for k in 0..len {
            let elem = crate::index_any::__torajs_any_index_get(items, k);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                return VALUE_UNDEFINED;
            }
            let stored = match map_elem(map_pair, this_arg, elem, k) {
                Ok(v) => store_elem(&mut product, k, v),
                Err(()) => false,
            };
            if !stored {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                return VALUE_UNDEFINED;
            }
        }
        write_product_length(&mut product, len);
        product
    }
}
