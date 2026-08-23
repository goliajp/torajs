//! §25.1.6 `ArrayBuffer.prototype` and §23.2.3 `%TypedArray%.prototype`
//! methods on the any-lane method dispatch
//! (RFC 20260823-typedarray-substrate 刀 1 and 刀 5).
//!
//! Every one of these takes its arguments as raw AnyValues because
//! every one of them coerces with steps that can run user code; the
//! substrate does the coercion so that the spec's step order (which
//! interleaves the coercions with detach and range checks) lives in
//! one place.
//!
//! Missing argument slots arrive as `undefined`, which is what the
//! spec says an absent one means for all of them — `fill`'s `end`
//! defaults to the length whether it was omitted or passed
//! explicitly, and `at()` coerces `undefined` to index 0.

use crate::nanbox::AnyValue;
use crate::nanbox::VALUE_UNDEFINED;
use crate::nanbox::box_double;

unsafe extern "C" {
    fn __torajs_arraybuffer_slice(av: AnyValue, start: AnyValue, end: AnyValue) -> AnyValue;
    fn __torajs_arraybuffer_resize(av: AnyValue, new_length: AnyValue);
    fn __torajs_typedarray_at(recv: AnyValue, index: AnyValue) -> AnyValue;
    fn __torajs_typedarray_fill(
        recv: AnyValue,
        value: AnyValue,
        start: AnyValue,
        end: AnyValue,
    ) -> AnyValue;
    fn __torajs_typedarray_copy_within(
        recv: AnyValue,
        target: AnyValue,
        start: AnyValue,
        end: AnyValue,
    ) -> AnyValue;
    fn __torajs_typedarray_reverse(recv: AnyValue) -> AnyValue;
    fn __torajs_typedarray_index_of(recv: AnyValue, search: AnyValue, from: AnyValue) -> f64;
    fn __torajs_typedarray_last_index_of(
        recv: AnyValue,
        search: AnyValue,
        from: AnyValue,
        has_from: i64,
    ) -> f64;
    fn __torajs_typedarray_includes(recv: AnyValue, search: AnyValue, from: AnyValue) -> AnyValue;
    fn __torajs_typedarray_subarray(recv: AnyValue, start: AnyValue, end: AnyValue) -> AnyValue;
    fn __torajs_typedarray_slice(recv: AnyValue, start: AnyValue, end: AnyValue) -> AnyValue;
    fn __torajs_typedarray_to_reversed(recv: AnyValue) -> AnyValue;
    fn __torajs_typedarray_with(recv: AnyValue, index: AnyValue, value: AnyValue) -> AnyValue;
    fn __torajs_typedarray_set(recv: AnyValue, source: AnyValue, offset: AnyValue) -> AnyValue;
}

/// `None` = this mid is not one ArrayBuffer.prototype owns, and the
/// caller raises its usual no-such-method TypeError.
///
/// # Safety
/// `recv` is a live ArrayBuffer AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn arraybuffer_method(
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
    match mid {
        torajs_rc::ANY_METHOD_SLICE => {
            Some(unsafe { __torajs_arraybuffer_slice(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_RESIZE => {
            unsafe { __torajs_arraybuffer_resize(recv, arg(0)) };
            Some(VALUE_UNDEFINED)
        }
        _ => None,
    }
}

/// §23.2.3 `%TypedArray%.prototype`, slab A. `None` is a mid this
/// prototype does not own, and the caller raises its usual
/// no-such-method TypeError — the same miss shape every other tag's
/// arm answers with.
///
/// The mutators answer the receiver itself (`fill` and `copyWithin`
/// return `O`, §23.2.3.9 step 12 / §23.2.3.6 step 16), so the
/// receiver is BORROWED here and handed straight back; a returned
/// reference is minted by whoever consumes it, exactly as the array
/// mutators already do.
///
/// # Safety
/// `recv` is a live TypedArray AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn typedarray_method(
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
    match mid {
        torajs_rc::ANY_METHOD_AT => Some(unsafe { __torajs_typedarray_at(recv, arg(0)) }),
        torajs_rc::ANY_METHOD_FILL => {
            Some(unsafe { __torajs_typedarray_fill(recv, arg(0), arg(1), arg(2)) })
        }
        torajs_rc::ANY_METHOD_COPY_WITHIN => {
            Some(unsafe { __torajs_typedarray_copy_within(recv, arg(0), arg(1), arg(2)) })
        }
        torajs_rc::ANY_METHOD_REVERSE => Some(unsafe { __torajs_typedarray_reverse(recv) }),
        torajs_rc::ANY_METHOD_INDEX_OF => Some(box_double(unsafe {
            __torajs_typedarray_index_of(recv, arg(0), arg(1))
        })),
        // An ABSENT `fromIndex` starts the scan at `len - 1`; an
        // explicit `undefined` coerces to 0 and looks at index 0
        // alone (§23.2.3.19 step 4). The two are the same slot here
        // and different answers there, so `argc` has to travel.
        torajs_rc::ANY_METHOD_LAST_INDEX_OF => Some(box_double(unsafe {
            __torajs_typedarray_last_index_of(recv, arg(0), arg(1), i64::from(argc > 1))
        })),
        torajs_rc::ANY_METHOD_INCLUDES => {
            Some(unsafe { __torajs_typedarray_includes(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_SUBARRAY => {
            Some(unsafe { __torajs_typedarray_subarray(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_SLICE => {
            Some(unsafe { __torajs_typedarray_slice(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_TO_REVERSED => Some(unsafe { __torajs_typedarray_to_reversed(recv) }),
        torajs_rc::ANY_METHOD_WITH => {
            Some(unsafe { __torajs_typedarray_with(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_SET => Some(unsafe { __torajs_typedarray_set(recv, arg(0), arg(1)) }),
        // §23.2.3.29 / §23.2.3.34 — the ordering is an AnyValue walk
        // over a user comparator, so it lives on this side; see
        // [`crate::method_call_buffer_sort`].
        torajs_rc::ANY_METHOD_SORT => {
            Some(unsafe { crate::method_call_buffer_sort::typedarray_sort(recv, argv, argc, true) })
        }
        torajs_rc::ANY_METHOD_TO_SORTED => Some(unsafe {
            crate::method_call_buffer_sort::typedarray_sort(recv, argv, argc, false)
        }),
        // §23.2.3.16 delegates to the array join — same six steps,
        // and that walk already answers the encoding question a
        // second copy would have to answer again.
        torajs_rc::ANY_METHOD_JOIN => {
            Some(unsafe { crate::method_call_buffer_join::typedarray_join(recv, argv, argc) })
        }
        // §23.2.3.31 is `Array.prototype.toString`, which is
        // `join()` with no separator — the argument list is dropped
        // rather than forwarded, so `ta.toString("-")` is still
        // comma-separated.
        torajs_rc::ANY_METHOD_TO_STRING => Some(unsafe {
            crate::method_call_buffer_join::typedarray_join(recv, core::ptr::null(), 0)
        }),
        _ => None,
    }
}
