//! §23.1.2.1 Array.from as a RUNTIME kernel (RFC
//! 20260808-construct-channel B6) — the any-tier walk a detached
//! `const f = Array.from; f(items, mapFn?, thisArg?)` call runs, and
//! the single source the typed lowering's escape shapes route to.
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
//!   is read at its turn, not snapshotted up front.
//!
//! Ownership: the walk's out-slot hands OWNED elements; a mapfn call
//! consumes the element (released here after the call, the callee
//! borrows argv) and answers an owned mapped value; pushing transfers
//! that stake into the array slot. The answer is a fresh rc-1
//! `Array<Any>` boxed as an owned AnyValue.

use core::ffi::c_void;

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

/// §23.1.2.1 Array.from(items, mapfn?, thisArg?) with no constructor
/// `this` (a detached call binds `this = undefined`, so step 8/12's
/// IsConstructor(C) is false and both branches take ArrayCreate).
/// Answers an owned boxed `Array<Any>`; a pending throw (non-callable
/// mapfn, non-iterable Get failure, mapfn body throw) answers
/// undefined with the throw recorded.
///
/// # Safety
/// `items` / `mapfn` / `this_arg` are live AnyValues borrowed for the
/// duration of the call.
pub(crate) unsafe fn array_from_plain(
    items: AnyValue,
    mapfn: AnyValue,
    this_arg: AnyValue,
) -> AnyValue {
    unsafe {
        // Step 2 — mapping + IsCallable BEFORE any iteration.
        let map_pair = if is_undefined(mapfn) {
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
                release_walk(arr, iter_slot);
                return VALUE_UNDEFINED;
            }
            if live == 0 {
                break;
            }
            // out is OWNED. Step 3.e.iii / 5.e.iii — Call(mapfn, T,
            // «kValue, k»): exactly two args whatever the declared
            // arity (the boxed lane materializes `arguments` off the
            // real argc), thisArg on the receiver channel.
            let v = if let Some((env, entry)) = map_pair {
                let call_argv = [out, box_int32(k as i32) as u64];
                let mapped = invoke_with_this(env, entry, this_arg, call_argv.as_ptr(), 2);
                crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
                if __torajs_throw_check() != 0 {
                    release_walk(arr, iter_slot);
                    return VALUE_UNDEFINED;
                }
                mapped
            } else {
                out
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

/// Abrupt-exit release: the partial product and whatever the walk
/// parked in `iter_slot`.
unsafe fn release_walk(arr: *mut u8, iter_slot: AnyValue) {
    unsafe {
        crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(arr as *mut c_void));
        crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
    }
}
