//! §23.2.3 the iteration family on `%TypedArray%.prototype`
//! (RFC 20260823-typedarray-substrate 刀 6, slab B) — `forEach`,
//! `map`, `filter`, `every`, `some`, the four `find*`, and the two
//! `reduce*`.
//!
//! They live here rather than in `torajs-buffer` for the reason
//! `sort` does: every one of them is a walk that calls user code,
//! and the boxed dual entry, the receiver-first channel and the
//! pending-throw check are all on this side. The buffer crate is
//! asked only two things — how long is the view right now, and what
//! is at index k.
//!
//! Three things separate these from their `Array.prototype` twins.
//!
//! **The order of the first two checks is inverted.** §23.2.3.15
//! step 2 validates the receiver, and only step 4 asks whether the
//! callback is callable — so `detached.forEach(3)` reports the
//! buffer, while `[].sort(3)` reports the comparator. The array
//! family has no step 2 to get in front.
//!
//! **The length is read once and never again.** Step 3 takes it
//! from the validated record; the callback is free to detach or
//! shrink the buffer afterwards, and the walk keeps going to the
//! length it was told. That is not a stale read left in by
//! accident — §10.4.5 makes the out-of-range `Get` answer
//! `undefined` rather than throw, precisely so this walk has
//! something to do for the rest of its rounds.
//!
//! **The destination is a typed view, so writing to it coerces.**
//! `map` stores through the same `Set` an assignment uses, which
//! runs ToNumber (or ToBigInt) on whatever the callback answered —
//! and that can throw, or run more user code.
//!
//! `map` and `filter` build their result with
//! TypedArrayCreateSameType rather than TypedArraySpeciesCreate:
//! `@@species` is out of this RFC, and slab A's `slice` /
//! `subarray` already answer that way.

use core::ffi::c_void;

use crate::method_call::not_callable;
use crate::method_call_closure_dispatch::{closure_boxed_entry, invoke_boxed, recv_first_shift};
use crate::nanbox::{AnyValue, VALUE_FALSE, VALUE_TRUE, VALUE_UNDEFINED, box_double};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_rc_dec(v: AnyValue);
    fn __torajs_anyv_rc_inc(v: AnyValue);
    fn __torajs_anyv_to_bool(v: AnyValue) -> bool;
    /// §23.2.4.4 at the ABI — the length, or -1 with a pending throw.
    fn __torajs_typedarray_validate(av: AnyValue) -> i64;
    fn __torajs_typedarray_create_same_type(av: AnyValue, len: i64) -> AnyValue;
    fn __torajs_typedarray_index_get(av: AnyValue, index: f64) -> AnyValue;
    fn __torajs_typedarray_index_set(av: AnyValue, index: f64, value: AnyValue);
}

/// The callback, resolved once per walk.
struct Callback {
    env: *mut c_void,
    entry: u64,
    /// argv slot shift for a receiver-first closure — hoisted out of
    /// the loop, one flags read per walk.
    shift: usize,
    this_arg: AnyValue,
}

impl Callback {
    /// `(kValue, k, O)` — the triple every one of these passes.
    /// `None` = the call threw and the walk stops; the return is
    /// owned by the caller otherwise.
    ///
    /// # Safety
    /// `value` and `recv` are live AnyValues the caller keeps alive
    /// across the call.
    unsafe fn call(&self, value: AnyValue, k: i64, recv: AnyValue) -> Option<AnyValue> {
        unsafe {
            let s = self.shift;
            let mut args = [VALUE_UNDEFINED; 8];
            if s == 1 {
                args[0] = self.this_arg;
            }
            args[s] = value;
            args[s + 1] = box_double(k as f64);
            args[s + 2] = recv;
            let r = invoke_boxed(self.env, self.entry, args.as_ptr(), (3 + s) as i64);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(r);
                return None;
            }
            Some(r)
        }
    }
}

/// Steps 2-4, in that order: validate the receiver, take the length
/// once, then ask whether the callback is callable.
///
/// `None` leaves a pending throw already raised.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `arg` reads the caller's
/// live argument slots.
unsafe fn prologue(recv: AnyValue, arg: &dyn Fn(i64) -> AnyValue) -> Option<(i64, Callback)> {
    let len = unsafe { __torajs_typedarray_validate(recv) };
    if len < 0 {
        return None;
    }
    let (env, entry) = match unsafe { closure_boxed_entry(arg(0)) } {
        Some(pair) => pair,
        None => {
            unsafe { not_callable() };
            return None;
        }
    };
    let shift = unsafe { recv_first_shift(env) };
    Some((
        len,
        Callback {
            env,
            entry,
            shift,
            this_arg: arg(1),
        },
    ))
}

/// §23.2.3.15 forEach / §23.2.3.7 every / §23.2.3.28 some — one
/// walk, three answers. `mode`: 0 = forEach, 1 = every, 2 = some.
///
/// # Safety
/// See [`typedarray_iter_method`].
unsafe fn walk_predicate(recv: AnyValue, len: i64, cb: &Callback, mode: u8) -> AnyValue {
    unsafe {
        for k in 0..len {
            let v = __torajs_typedarray_index_get(recv, k as f64);
            let Some(r) = cb.call(v, k, recv) else {
                __torajs_anyv_rc_dec(v);
                return VALUE_UNDEFINED;
            };
            __torajs_anyv_rc_dec(v);
            let truthy = __torajs_anyv_to_bool(r);
            __torajs_anyv_rc_dec(r);
            match mode {
                1 if !truthy => return VALUE_FALSE,
                2 if truthy => return VALUE_TRUE,
                _ => {}
            }
        }
        match mode {
            1 => VALUE_TRUE,
            2 => VALUE_FALSE,
            _ => VALUE_UNDEFINED,
        }
    }
}

/// §23.2.3.11-14 via FindViaPredicate. `mode`: 0 = find,
/// 1 = findIndex, 2 = findLast, 3 = findLastIndex. The last two walk
/// backwards. On the hit `find` / `findLast` transfer the owned
/// element read as the return; the index forms release it.
///
/// # Safety
/// See [`typedarray_iter_method`].
unsafe fn walk_find(recv: AnyValue, len: i64, cb: &Callback, mode: u8) -> AnyValue {
    unsafe {
        let backwards = mode >= 2;
        for step in 0..len {
            let k = if backwards { len - 1 - step } else { step };
            let v = __torajs_typedarray_index_get(recv, k as f64);
            let Some(r) = cb.call(v, k, recv) else {
                __torajs_anyv_rc_dec(v);
                return VALUE_UNDEFINED;
            };
            let truthy = __torajs_anyv_to_bool(r);
            __torajs_anyv_rc_dec(r);
            if truthy {
                if mode == 0 || mode == 2 {
                    return v;
                }
                __torajs_anyv_rc_dec(v);
                return box_double(k as f64);
            }
            __torajs_anyv_rc_dec(v);
        }
        // §23.2.4.8 step 3 — no hit answers `undefined` for the
        // value forms and -1 for the index forms.
        if mode == 0 || mode == 2 {
            VALUE_UNDEFINED
        } else {
            box_double(-1.0)
        }
    }
}

/// §23.2.3.20 map — a same-type view of the callback's answers.
/// Each store coerces (and can throw, and can run more user code).
///
/// # Safety
/// See [`typedarray_iter_method`].
unsafe fn walk_map(recv: AnyValue, len: i64, cb: &Callback) -> AnyValue {
    unsafe {
        // §23.2.3.20 step 6 TypedArraySpeciesCreate — the
        // constructor-face read runs BEFORE the loop.
        if crate::buffer_species::__torajs_buffer_species_guard(recv) != 0 {
            return VALUE_UNDEFINED;
        }
        let out = __torajs_typedarray_create_same_type(recv, len);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        for k in 0..len {
            let v = __torajs_typedarray_index_get(recv, k as f64);
            let Some(r) = cb.call(v, k, recv) else {
                __torajs_anyv_rc_dec(v);
                __torajs_anyv_rc_dec(out);
                return VALUE_UNDEFINED;
            };
            __torajs_anyv_rc_dec(v);
            __torajs_typedarray_index_set(out, k as f64, r);
            __torajs_anyv_rc_dec(r);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(out);
                return VALUE_UNDEFINED;
            }
        }
        out
    }
}

/// §23.2.3.10 filter — the kept elements are collected first and the
/// destination is sized from the count, so a callback that shrinks
/// the buffer cannot change how big the answer is.
///
/// # Safety
/// See [`typedarray_iter_method`].
unsafe fn walk_filter(recv: AnyValue, len: i64, cb: &Callback) -> AnyValue {
    unsafe {
        let mut kept: Vec<AnyValue> = Vec::new();
        for k in 0..len {
            let v = __torajs_typedarray_index_get(recv, k as f64);
            let Some(r) = cb.call(v, k, recv) else {
                __torajs_anyv_rc_dec(v);
                for held in kept {
                    __torajs_anyv_rc_dec(held);
                }
                return VALUE_UNDEFINED;
            };
            let selected = __torajs_anyv_to_bool(r);
            __torajs_anyv_rc_dec(r);
            if selected {
                kept.push(v);
            } else {
                __torajs_anyv_rc_dec(v);
            }
        }
        // §23.2.3.10 step 9 TypedArraySpeciesCreate — the
        // constructor-face read runs AFTER the callback loop.
        if crate::buffer_species::__torajs_buffer_species_guard(recv) != 0 {
            for v in kept {
                __torajs_anyv_rc_dec(v);
            }
            return VALUE_UNDEFINED;
        }
        let out = __torajs_typedarray_create_same_type(recv, kept.len() as i64);
        if __torajs_throw_check() == 0 {
            for (j, v) in kept.iter().enumerate() {
                __torajs_typedarray_index_set(out, j as f64, *v);
            }
        }
        for v in kept {
            __torajs_anyv_rc_dec(v);
        }
        out
    }
}

/// §23.2.3.23 reduce / §23.2.3.24 reduceRight. The accumulator is
/// the callback's first argument, so this walk does not use
/// [`Callback::call`] — its triple is `(acc, kValue, k, O)`.
///
/// # Safety
/// See [`typedarray_iter_method`].
unsafe fn walk_reduce(
    recv: AnyValue,
    len: i64,
    cb: &Callback,
    argv: *const u64,
    argc: i64,
    right: bool,
) -> AnyValue {
    unsafe {
        // Step 5 — an initialValue is an argc question, not an
        // undefined question: `reduce(f, undefined)` has one.
        let has_init = argc >= 2;
        if len == 0 && !has_init {
            __torajs_throw_type_error(c"reduce of empty array with no initial value".as_ptr());
            return VALUE_UNDEFINED;
        }
        let mut step = 0i64;
        let mut acc = if has_init {
            let init = *argv.add(1);
            __torajs_anyv_rc_inc(init);
            init
        } else {
            let k = if right { len - 1 } else { 0 };
            step = 1;
            __torajs_typedarray_index_get(recv, k as f64)
        };
        while step < len {
            let k = if right { len - 1 - step } else { step };
            let v = __torajs_typedarray_index_get(recv, k as f64);
            let s = cb.shift;
            let mut args = [VALUE_UNDEFINED; 8];
            if s == 1 {
                args[0] = cb.this_arg;
            }
            args[s] = acc;
            args[s + 1] = v;
            args[s + 2] = box_double(k as f64);
            args[s + 3] = recv;
            let r = invoke_boxed(cb.env, cb.entry, args.as_ptr(), (4 + s) as i64);
            __torajs_anyv_rc_dec(v);
            __torajs_anyv_rc_dec(acc);
            if __torajs_throw_check() != 0 {
                __torajs_anyv_rc_dec(r);
                return VALUE_UNDEFINED;
            }
            acc = r;
            step += 1;
        }
        acc
    }
}

/// slab B's share of §23.2.3. `None` is a mid this family does not
/// own — the caller keeps looking.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn typedarray_iter_method(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let arg = |i: i64| -> AnyValue {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    let mode = iter_mode(mid)?;
    unsafe {
        let Some((len, cb)) = prologue(recv, &arg) else {
            return Some(VALUE_UNDEFINED);
        };
        Some(match mode {
            IterMode::ForEach => walk_predicate(recv, len, &cb, 0),
            IterMode::Every => walk_predicate(recv, len, &cb, 1),
            IterMode::Some => walk_predicate(recv, len, &cb, 2),
            IterMode::Find(m) => walk_find(recv, len, &cb, m),
            IterMode::Map => walk_map(recv, len, &cb),
            IterMode::Filter => walk_filter(recv, len, &cb),
            IterMode::Reduce { right } => walk_reduce(recv, len, &cb, argv, argc, right),
        })
    }
}

enum IterMode {
    ForEach,
    Every,
    Some,
    Find(u8),
    Map,
    Filter,
    Reduce { right: bool },
}

fn iter_mode(mid: i64) -> Option<IterMode> {
    Some(match mid {
        torajs_rc::ANY_METHOD_FOR_EACH => IterMode::ForEach,
        torajs_rc::ANY_METHOD_EVERY => IterMode::Every,
        torajs_rc::ANY_METHOD_SOME => IterMode::Some,
        torajs_rc::ANY_METHOD_FIND => IterMode::Find(0),
        torajs_rc::ANY_METHOD_FIND_INDEX => IterMode::Find(1),
        torajs_rc::ANY_METHOD_FIND_LAST => IterMode::Find(2),
        torajs_rc::ANY_METHOD_FIND_LAST_INDEX => IterMode::Find(3),
        torajs_rc::ANY_METHOD_MAP => IterMode::Map,
        torajs_rc::ANY_METHOD_FILTER => IterMode::Filter,
        torajs_rc::ANY_METHOD_REDUCE => IterMode::Reduce { right: false },
        torajs_rc::ANY_METHOD_REDUCE_RIGHT => IterMode::Reduce { right: true },
        _ => return None,
    })
}
