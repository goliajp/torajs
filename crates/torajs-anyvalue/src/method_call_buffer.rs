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
    /// torajs-buffer — §25.1.6.13 [[ArrayBufferMaxByteLength]] probe.
    fn __torajs_arraybuffer_resizable(av: AnyValue) -> i64;
    fn __torajs_arraybuffer_slice(av: AnyValue, start: AnyValue, end: AnyValue) -> AnyValue;
    fn __torajs_arraybuffer_resize(av: AnyValue, new_length: AnyValue);
    fn __torajs_arraybuffer_transfer(
        av: AnyValue,
        new_length: AnyValue,
        preserve_resizability: i64,
    ) -> AnyValue;
    fn __torajs_typedarray_at(recv: AnyValue, index: AnyValue) -> AnyValue;
    /// §23.2.3's four `Uint8Array.prototype` text conversions. The
    /// `Uint8Array` brand check is theirs, not this dispatcher's —
    /// the heap tag is `TypedArray` for all eleven element types.
    fn __torajs_uint8array_to_base64(recv: AnyValue, options: AnyValue) -> AnyValue;
    fn __torajs_uint8array_to_hex(recv: AnyValue) -> AnyValue;
    fn __torajs_uint8array_set_from_base64(
        recv: AnyValue,
        string: AnyValue,
        options: AnyValue,
    ) -> AnyValue;
    fn __torajs_uint8array_set_from_hex(recv: AnyValue, string: AnyValue) -> AnyValue;
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
    /// §23.2.4.4 at the ABI — the length, or -1 with a pending throw.
    fn __torajs_typedarray_validate(recv: AnyValue) -> i64;
    fn __torajs_arr_iter_create_keys(src: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn __torajs_arr_iter_create_values(src: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    fn __torajs_arr_iter_create_entries(src: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
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
        // §25.1.6.13 — the reified `get resizable` invoked through
        // `.call(recv)`; the id never interns (the GET_SIZE
        // posture), so only the carried-mid re-dispatch lands here.
        torajs_rc::ANY_METHOD_GET_RESIZABLE => Some(unsafe {
            crate::nanbox_encode::__torajs_anyv_box_from_pair(
                1,
                (__torajs_arraybuffer_resizable(recv) != 0) as i64,
            )
        }),
        torajs_rc::ANY_METHOD_SLICE => {
            Some(unsafe { __torajs_arraybuffer_slice(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_RESIZE => {
            unsafe { __torajs_arraybuffer_resize(recv, arg(0)) };
            Some(VALUE_UNDEFINED)
        }
        // §25.1.6.7 / §25.1.6.8 — one body, two names; the flag is
        // the whole difference (`transfer` keeps a resizable buffer
        // resizable, `transferToFixedLength` does not).
        torajs_rc::ANY_METHOD_TRANSFER => {
            Some(unsafe { __torajs_arraybuffer_transfer(recv, arg(0), 1) })
        }
        torajs_rc::ANY_METHOD_TRANSFER_TO_FIXED_LENGTH => {
            Some(unsafe { __torajs_arraybuffer_transfer(recv, arg(0), 0) })
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
        torajs_rc::ANY_METHOD_TO_BASE64 => {
            Some(unsafe { __torajs_uint8array_to_base64(recv, arg(0)) })
        }
        torajs_rc::ANY_METHOD_TO_HEX => Some(unsafe { __torajs_uint8array_to_hex(recv) }),
        torajs_rc::ANY_METHOD_SET_FROM_BASE64 => {
            Some(unsafe { __torajs_uint8array_set_from_base64(recv, arg(0), arg(1)) })
        }
        torajs_rc::ANY_METHOD_SET_FROM_HEX => {
            Some(unsafe { __torajs_uint8array_set_from_hex(recv, arg(0)) })
        }
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
        // §23.2.5.1-3 — `values` / `keys` / `entries` answer the
        // SAME Array Iterator arrays do; CreateArrayIterator's own
        // closure branches on the source having a
        // `[[TypedArrayName]]`, and that branch lives in the
        // iterator cell's step (`torajs_arr::iter`). Only the
        // up-front ValidateTypedArray is ours: `detached.values()`
        // throws before any cell is minted.
        //
        // §23.2.3.36 `[Symbol.iterator]` is the same function
        // object as `values`, so it interns to the same id and
        // needs no arm of its own.
        m if m == torajs_rc::ANY_METHOD_KEYS
            || m == torajs_rc::ANY_METHOD_VALUES
            || m == torajs_rc::ANY_METHOD_ENTRIES =>
        {
            if unsafe { __torajs_typedarray_validate(recv) } < 0 {
                return Some(VALUE_UNDEFINED);
            }
            let src = crate::nanbox::as_void_ptr(recv);
            // Fresh cell at rc=1; the owned-return protocol takes it
            // as-is, and the mint took its own share of the view.
            let it = unsafe {
                if m == torajs_rc::ANY_METHOD_KEYS {
                    __torajs_arr_iter_create_keys(src)
                } else if m == torajs_rc::ANY_METHOD_VALUES {
                    __torajs_arr_iter_create_values(src)
                } else {
                    __torajs_arr_iter_create_entries(src)
                }
            };
            Some(crate::nanbox::box_void_ptr(it))
        }
        // §23.2.3's iteration family — the walks that call user
        // code, in [`crate::method_call_buffer_iter`].
        _ => unsafe {
            crate::method_call_buffer_iter::typedarray_iter_method(recv, mid, argv, argc)
        },
    }
}
