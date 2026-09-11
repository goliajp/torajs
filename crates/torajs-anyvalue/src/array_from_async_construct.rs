//! `Array.fromAsync` constructor-`this` construct kernels (RFC
//! 20260808-construct-channel B6 刀 4, rotation 346; per-element
//! since rotation 634).
//!
//! The promise-side kernel (`torajs-promise/from_async.rs`) drives
//! the iteration; when the `.call(C, …)` receiver channel hands a
//! constructor `this`, the product is built HERE instead of the
//! plain `Array<Any>` build, in the spec's order. `begin` is
//! §2.1.1 steps 3.a–3.f.i / 3.g.i–ii: GetIterator with hint=async
//! (resolved and CALLED once, parked in the step slots the promise
//! side then drives), then `A = Construct(C)` for an iterator
//! source or `Construct(C, «len»)` for an array-like one — before
//! the first `next()`. `define` is one CreateDataPropertyOrThrow
//! (3.j.ii.10, sharing `array_from.rs`'s `store_elem`),
//! `finish_length` is `Set(A, "length", k, true)` (3.j.ii.7.a).
//!
//! The order is observable in two places, which is why rotation
//! 346's collect-then-finish shape (its recorded MVP boundary) is
//! gone: a define that fails must CLOSE the iterator (3.j.ii.11 —
//! an infinite iterator left open was a hang), and a constructor's
//! side effects run before the first step.

use crate::iter_any::USER_ITERATOR_LANE;
use crate::iter_any_get_method::{GetIterator, get_async_iterator, get_iterator};
use crate::method_call_arr_species::{store_elem, write_product_length};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_int32};

unsafe extern "C" {
    fn __torajs_throw_check() -> i64;
}

/// What §2.1.1 step 3 found the source to be.
enum Source {
    /// Steps 3.d / 3.e — an iterator (user-resolved and parked, or a
    /// builtin lane the cascade claims on its first step).
    Iterator,
    /// Step 3.g — no iterator anywhere; the once-read
    /// LengthOfArrayLike.
    ArrayLike(i32),
}

/// §7.4.2 GetIterator hint=async in the exact order the cascade's
/// first step probes: `@@asyncIterator`, then `@@iterator`, then
/// the builtin lanes. A user iterator parks under
/// `USER_ITERATOR_LANE` so that first step finds it instead of
/// resolving (and calling) the method twice; an array-like source
/// parks its length the same way the walk itself would, so the
/// walk does not re-read it. `None` = a probe threw.
///
/// # Safety
/// `items` is a live AnyValue; the slots are valid and pristine.
unsafe fn resolve_source(
    items: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
) -> Option<Source> {
    unsafe {
        for probe in [get_async_iterator, get_iterator] {
            match probe(items) {
                GetIterator::Threw => return None,
                GetIterator::Iterator(it) => {
                    *iter_slot = it;
                    *idx_slot = USER_ITERATOR_LANE;
                    return Some(Source::Iterator);
                }
                GetIterator::NoUserMethod => {}
            }
        }
        if crate::iter_any::claims_iterable(items) {
            return Some(Source::Iterator);
        }
        let len = crate::iter_any_array_like::array_like_length(items);
        if __torajs_throw_check() != 0 {
            return None;
        }
        *iter_slot = box_int32(len);
        Some(Source::ArrayLike(len))
    }
}

/// Steps 3.a–3.g.ii: resolve the source, then Construct. Answers the
/// OWNED product, or undefined with the throw pending. `this_c` and
/// `items` are BORROWED; the step slots are the promise side's,
/// pristine on entry, and whatever parks in them is the caller's to
/// release at loop exit (the same contract as every other consumer
/// of the step protocol).
///
/// # Safety
/// `this_c` / `items` are live for the duration of the call;
/// `idx_slot` / `iter_slot` are valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_from_async_construct_begin(
    this_c: AnyValue,
    items: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
) -> AnyValue {
    unsafe {
        let len_box;
        let (argv, argc): (*const AnyValue, i64) = match resolve_source(items, idx_slot, iter_slot)
        {
            None => return VALUE_UNDEFINED,
            Some(Source::Iterator) => (core::ptr::null(), 0),
            Some(Source::ArrayLike(len)) => {
                len_box = [crate::nanbox_encode::__torajs_anyv_box_i64(len as i64)];
                (len_box.as_ptr(), 1)
            }
        };
        let product = crate::construct::__torajs_anyv_construct(this_c, argv, argc);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        product
    }
}

/// Step 3.j.ii.10 / 3.g.vi.5 — CreateDataPropertyOrThrow(A,
/// ! ToString(k), value). `elem` is OWNED and consumed on every
/// path; the product slot is threaded so an Arr product's resize
/// relocation writes the fresh cell back. 1 on success, 0 with the
/// throw pending — the caller owes the iterator a close.
///
/// # Safety
/// `product` points at the live owned product; `elem` is a live
/// owned AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_from_async_define(
    product: *mut AnyValue,
    k: i64,
    elem: AnyValue,
) -> i64 {
    unsafe { store_elem(&mut *product, k, elem) as i64 }
}

/// Step 3.j.ii.7.a / 3.g.vii — `Set(A, "length", k, true)`.
///
/// # Safety
/// `product` points at the live owned product.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_from_async_finish_length(product: *mut AnyValue, k: i64) {
    unsafe { write_product_length(&mut *product, k) }
}
