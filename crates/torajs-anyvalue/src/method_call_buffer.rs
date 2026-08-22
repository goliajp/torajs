//! §25.1.6 `ArrayBuffer.prototype` methods on the any-lane method
//! dispatch (RFC 20260823-typedarray-substrate 刀 1).
//!
//! `slice` and `resize` both take their arguments as raw AnyValues
//! because both coerce them with steps that can run user code; the
//! substrate does the coercion so that the spec's step order (which
//! interleaves the coercions with detach and range checks) lives in
//! one place.

use crate::nanbox::AnyValue;
use crate::nanbox::VALUE_UNDEFINED;

unsafe extern "C" {
    fn __torajs_arraybuffer_slice(av: AnyValue, start: AnyValue, end: AnyValue) -> AnyValue;
    fn __torajs_arraybuffer_resize(av: AnyValue, new_length: AnyValue);
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
