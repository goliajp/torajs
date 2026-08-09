//! `Array.fromAsync` constructor-`this` finish kernel (RFC
//! 20260808-construct-channel B6 刀 4 真身, rotation 346).
//!
//! The promise-side kernels (`torajs-promise/from_async.rs`) collect
//! and settle the elements; when the `.call(C, …)` receiver channel
//! hands a constructor `this`, the settled boxes finish HERE instead
//! of the plain `Array<Any>` build: `A = Construct(C)` (spec-iterable
//! source, §3.j — no arguments) or `Construct(C, «len»)` (array-like,
//! §3.k.iv), one CreateDataPropertyOrThrow per element, then
//! `Set(A, "length", n, true)` — the exact walk `array_from.rs` does
//! for the sync static, sharing its `store_elem` /
//! `write_product_length` kernels.
//!
//! Recorded MVP boundary (same posture as the promise side's
//! sync-source note): Construct runs after the elements settle, not
//! before iteration — the observable count/argument asserts hold; a
//! constructor whose side effects race the element awaits would see
//! the difference.

use crate::method_call_arr_species::{store_elem, write_product_length};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
}

/// Finish a constructor-`this` fromAsync: `this_c` and `items` are
/// BORROWED; the `n` boxes at `elems` are OWNED and transfer in
/// (consumed on every path). Answers an OWNED product, or undefined
/// with the throw pending.
///
/// # Safety
/// `elems` points at `n` live owned AnyValues; `this_c` / `items`
/// are live for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_from_async_construct_finish(
    this_c: AnyValue,
    items: AnyValue,
    elems: *const AnyValue,
    n: i64,
) -> AnyValue {
    unsafe {
        let release_from = |i: usize| {
            for j in i..n as usize {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(*elems.add(j));
            }
        };
        // §3.j vs §3.k — a spec-iterable source constructs with no
        // arguments; the array-like branch hands «len» (== n, the
        // collect walked every index).
        let len_box;
        let (argv, argc): (*const AnyValue, i64) = if crate::iter_any::claims_iterable(items) {
            (core::ptr::null(), 0)
        } else {
            len_box = [crate::nanbox_encode::__torajs_anyv_box_i64(n)];
            (len_box.as_ptr(), 1)
        };
        let mut product = crate::construct::__torajs_anyv_construct(this_c, argv, argc);
        if __torajs_throw_check() != 0 {
            release_from(0);
            return VALUE_UNDEFINED;
        }
        for i in 0..n as usize {
            if !store_elem(&mut product, i as i64, *elems.add(i)) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(product);
                release_from(i + 1);
                return VALUE_UNDEFINED;
            }
        }
        write_product_length(&mut product, n);
        product
    }
}
