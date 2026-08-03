//! `__torajs_reflect_construct` — §28.1.2 Reflect.construct(target,
//! argumentsList[, newTarget]) (rotation 293).
//!
//! The runtime-construct kernel's reflective face: IsConstructor
//! gates on target AND newTarget (§28.1.2 steps 1/3 — the lowering
//! passes target again for the two-argument form), then an
//! unconditional CreateListFromArrayLike (§28.1.2 step 4 — nullish
//! is a TypeError here, same posture as `Reflect.apply`), then the
//! same factory-adapter construct `__torajs_anyv_construct` runs.
//!
//! When newTarget differs from target, the constructed object's
//! [[Prototype]] comes from `Get(newTarget, "prototype")` (§9.1.13
//! OrdinaryCreateFromConstructor reads newTarget, not target) — the
//! factory adapter wires target's prototype, so the kernel re-wires
//! afterward through the ordinary [[SetPrototypeOf]]. A non-object
//! `prototype` answer keeps the factory wiring (%Object.prototype%
//! fallback is what the factory already gave). Recorded boundary: a
//! fixed-layout (struct-repr) instance refuses [[SetPrototypeOf]]
//! loudly, so the differing-newTarget form raises that TypeError
//! today instead of answering a mis-wired object.
//!
//! The list walk is the cold-path copy of
//! [`crate::method_call_closure_apply_like::apply_list`]'s two lanes
//! (dense Arr / array-like [[Get]]) collecting into a Vec — the
//! apply kernel's stack-buffer hot shape stays untouched.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::construct::{ctor_entry, is_constructor};
use crate::iter_any_result::iter_result_get;
use crate::method_call_closure_dispatch::invoke_boxed;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-arr — kind-aware boxed element read (owned +1).
    fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64;
    /// torajs-meta — ordinary [[SetPrototypeOf]] over an any pair.
    fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64);
}

/// Arr cell length slot — mirror of torajs-arr `layout::ARR_LEN_OFF`.
const ARR_LEN_OFF: usize = 8;

/// §7.3.18 CreateListFromArrayLike → owned boxed elements. `None`
/// means a TypeError / abrupt [[Get]] is already recorded pending
/// (collected elements released).
unsafe fn collect_list(list: AnyValue) -> Option<Vec<u64>> {
    unsafe {
        let ptr = if is_cell(list) {
            as_void_ptr(list)
        } else {
            core::ptr::null_mut()
        };
        let tag = if ptr.is_null() {
            0
        } else {
            (ptr.cast::<u8>().add(4) as *const u16).read()
        };
        if ptr.is_null()
            || tag == Tag::Str as u16
            || tag == Tag::Symbol as u16
            || tag == Tag::BigInt as u16
        {
            // §7.3.18 step 2 — a primitive argumentsList (heap-celled
            // primitives included) is a TypeError.
            __torajs_throw_type_error(
                c"Reflect.construct requires the second argument be an object".as_ptr(),
            );
            return None;
        }
        if tag == Tag::Arr as u16 {
            let n = *(ptr.cast::<u8>().add(ARR_LEN_OFF) as *const u64) as usize;
            return Some(
                (0..n)
                    .map(|i| __torajs_arr_index_get(ptr, i as i64))
                    .collect(),
            );
        }
        // Array-like: ToLength(Get(obj, "length")), then per-index
        // Get — accessor-aware, abrupt completions forward.
        let len_v = iter_result_get(list, ptr, b"length")?;
        let (t, p) = (
            crate::__torajs_anyv_unbox_tag(len_v),
            crate::__torajs_anyv_unbox_value(len_v),
        );
        let n_f = crate::coerce::any_to_number(t, p);
        __torajs_anyv_rc_dec(len_v);
        if __torajs_throw_check() != 0 {
            return None;
        }
        let n = if n_f.is_nan() || n_f <= 0.0 {
            0
        } else {
            n_f.trunc().min(9007199254740991.0) as usize
        };
        let mut boxed: Vec<u64> = Vec::with_capacity(n);
        for i in 0..n {
            let key = i.to_string();
            let Some(v) = iter_result_get(list, ptr, key.as_bytes()) else {
                for b in &boxed {
                    __torajs_anyv_rc_dec(*b);
                }
                return None;
            };
            boxed.push(v);
        }
        Some(boxed)
    }
}

/// See module doc. Answers the constructed object, owned; on a gate
/// TypeError the pending throw records and `undefined` rides back.
///
/// # Safety
/// Cell operands are valid heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_reflect_construct(
    target: AnyValue,
    list: AnyValue,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        // §28.1.2 step 1 — IsConstructor(target).
        if !is_constructor(target) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the first argument be a constructor".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §28.1.2 step 3 — IsConstructor(newTarget). The lowering
        // hands target back for the two-argument form.
        if !is_constructor(new_target) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the third argument be a constructor".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        // §28.1.2 step 4 — CreateListFromArrayLike, unconditional
        // (nullish is not an empty list here).
        if is_undefined(list) || is_null(list) {
            __torajs_throw_type_error(
                c"Reflect.construct requires the second argument be an object".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let Some(boxed) = collect_list(list) else {
            return VALUE_UNDEFINED;
        };
        let entry = ctor_entry(as_void_ptr(target) as u64);
        if entry == 0 {
            for b in &boxed {
                __torajs_anyv_rc_dec(*b);
            }
            __torajs_throw_type_error(
                c"this constructor cannot be reached through a runtime value yet".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let obj = invoke_boxed(
            core::ptr::null_mut(),
            entry,
            boxed.as_ptr(),
            boxed.len() as i64,
        );
        for b in boxed {
            __torajs_anyv_rc_dec(b);
        }
        if __torajs_throw_check() != 0 {
            return obj;
        }
        // §9.1.13 — [[Prototype]] from newTarget when it differs.
        if new_target != target && is_cell(obj) {
            if let Some(proto) = iter_result_get(new_target, as_void_ptr(new_target), b"prototype")
            {
                if is_cell(proto) {
                    __torajs_anyv_set_prototype_of(obj, proto);
                }
                __torajs_anyv_rc_dec(proto);
            } else {
                // Abrupt Get(newTarget, "prototype") — release the
                // constructed object and forward the throw.
                __torajs_anyv_rc_dec(obj);
                return VALUE_UNDEFINED;
            }
        }
        obj
    }
}
