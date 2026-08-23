//! §25.3.4 `DataView.prototype` methods on the any-lane method
//! dispatch (RFC 20260823-typedarray-substrate 刀 7).
//!
//! Twenty-two method names, two kernels: each mid folds to
//! `(is_set, element kind)` here and the kernels own the §25.3.1.1
//! GetViewValue / SetViewValue step order (coercions before the
//! bounds reads, endianness per call).

use torajs_rc::any_method::*;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_dataview_get(
        recv: AnyValue,
        kind: i64,
        offset: AnyValue,
        little_endian: AnyValue,
    ) -> AnyValue;
    fn __torajs_dataview_set(
        recv: AnyValue,
        kind: i64,
        offset: AnyValue,
        value: AnyValue,
        little_endian: AnyValue,
    );
}

/// `(is_set, Kind discriminant)` for a DataView mid, or `None` when
/// the mid is not one the prototype owns. The discriminants mirror
/// torajs-buffer `typedarray::Kind` (wire format, append-only).
pub(crate) fn dataview_mid(mid: i64) -> Option<(bool, i64)> {
    Some(match mid {
        ANY_METHOD_DV_GET_INT8 => (false, 0),
        ANY_METHOD_DV_SET_INT8 => (true, 0),
        ANY_METHOD_DV_GET_UINT8 => (false, 1),
        ANY_METHOD_DV_SET_UINT8 => (true, 1),
        ANY_METHOD_DV_GET_INT16 => (false, 3),
        ANY_METHOD_DV_SET_INT16 => (true, 3),
        ANY_METHOD_DV_GET_UINT16 => (false, 4),
        ANY_METHOD_DV_SET_UINT16 => (true, 4),
        ANY_METHOD_DV_GET_INT32 => (false, 5),
        ANY_METHOD_DV_SET_INT32 => (true, 5),
        ANY_METHOD_DV_GET_UINT32 => (false, 6),
        ANY_METHOD_DV_SET_UINT32 => (true, 6),
        ANY_METHOD_DV_GET_FLOAT16 => (false, 11),
        ANY_METHOD_DV_SET_FLOAT16 => (true, 11),
        ANY_METHOD_DV_GET_FLOAT32 => (false, 7),
        ANY_METHOD_DV_SET_FLOAT32 => (true, 7),
        ANY_METHOD_DV_GET_FLOAT64 => (false, 8),
        ANY_METHOD_DV_SET_FLOAT64 => (true, 8),
        ANY_METHOD_DV_GET_BIGINT64 => (false, 9),
        ANY_METHOD_DV_SET_BIGINT64 => (true, 9),
        ANY_METHOD_DV_GET_BIGUINT64 => (false, 10),
        ANY_METHOD_DV_SET_BIGUINT64 => (true, 10),
        _ => return None,
    })
}

/// `None` = this mid is not one DataView.prototype owns, and the
/// caller raises its usual no-such-method TypeError.
///
/// # Safety
/// `recv` is a live DataView AnyValue; `argv` holds `argc` live
/// AnyValues.
pub(crate) unsafe fn dataview_method(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let (is_set, kind) = dataview_mid(mid)?;
    let arg = |i: i64| -> AnyValue {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    if is_set {
        unsafe { __torajs_dataview_set(recv, kind, arg(0), arg(1), arg(2)) };
        Some(VALUE_UNDEFINED)
    } else {
        Some(unsafe { __torajs_dataview_get(recv, kind, arg(0), arg(1)) })
    }
}
