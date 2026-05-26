//! NaN-box encode + decode-tag/value FFI shims —
//! `__torajs_anyv_box_*` + `__torajs_anyv_unbox_tag` +
//! `__torajs_anyv_unbox_value` (Step 7b / 7d).
//!
//! Split out of [`crate::nanbox_ffi`] to keep that file under
//! the 500-line hard limit. The encode/unbox shims are the
//! migration entry points ssa_lower calls into; the rest
//! (rc_inc/dec, to_number/to_str/to_bool, strict_eq,
//! compare/arith/add) stays in `nanbox_ffi.rs`.

use std::ffi::c_void;

use torajs_rc::AnySlotTag;

use crate::nanbox::{
    AnyValue, VALUE_NULL, VALUE_UNDEFINED, as_bool, as_double, as_int32, box_bool, box_double,
    box_int32, box_void_ptr, is_bool, is_cell, is_double, is_int32, is_null, is_undefined,
};

// ============================================================
// Encode shims — let ssa_lower emit one-line calls instead of
// hand-rolling the bit-twiddle in IR. Cheap (every fn is one or
// two integer ops) but they make the migration in 7c-7f a pure
// callsite swap.
// ============================================================

/// Encode a 32-bit signed integer as an [`AnyValue`].
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_i32(x: i32) -> AnyValue {
    box_int32(x)
}

/// Encode an `f64` as an [`AnyValue`].
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_f64(x: f64) -> AnyValue {
    box_double(x)
}

/// Encode a `bool` (passed as `i64` so ssa_lower can pass its
/// zext-i64 representation directly; any nonzero value → true).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_bool(b: i64) -> AnyValue {
    box_bool(b != 0)
}

/// Encode a heap pointer as an [`AnyValue`]. Caller transfers
/// ownership of the rc — no `rc_inc` is performed here.
///
/// # Safety
///
/// `p` is null or a valid `*mut HeapHeader` whose top 16 bits are
/// zero (aarch64 user-VA, 48 bits).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_pointer(p: *mut c_void) -> AnyValue {
    if p.is_null() {
        return VALUE_NULL;
    }
    box_void_ptr(p)
}

/// Encode an `i64` as an [`AnyValue`]. Values that fit in `i32`
/// become tagged Int32 immediates; values outside that range
/// promote to `f64` (lossless for ±2^53; precision loss beyond
/// matches JS semantics — Number is f64-backed).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_box_i64(x: i64) -> AnyValue {
    if let Ok(n32) = i32::try_from(x) {
        box_int32(n32)
    } else {
        box_double(x as f64)
    }
}

/// Encode a legacy `(tag, value)` pair as an [`AnyValue`].
/// Bridge for ssa_lower sites where the tag is a runtime value
/// (forward-Any: `throw e`, `consume_any`) and not known at
/// compile time. The tag follows the [`AnySlotTag`] enum
/// numbering: 0=Null, 1=Bool, 2=I64, 3=F64, 4=Heap, 5=Undef.
///
/// # Safety
///
/// For `tag == 4` (Heap), `value` must be 0 or a valid
/// `*mut HeapHeader` (the caller transfers ownership of an rc).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> AnyValue {
    match tag {
        0 => VALUE_NULL,
        1 => box_bool(value != 0),
        2 => __torajs_anyv_box_i64(value),
        3 => box_double(f64::from_bits(value as u64)),
        4 => {
            if value == 0 {
                VALUE_NULL
            } else {
                box_void_ptr(value as *mut c_void)
            }
        }
        5 => VALUE_UNDEFINED,
        _ => VALUE_NULL,
    }
}

/// Return the `null` sentinel.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_null() -> AnyValue {
    VALUE_NULL
}

/// Return the `undefined` sentinel.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_undefined() -> AnyValue {
    VALUE_UNDEFINED
}

// ============================================================
// Decode shims — pair recovery for legacy callers
// ============================================================

/// Decode an [`AnyValue`] to its [`AnySlotTag`] enum index
/// (0=Null, 1=Bool, 2=I64, 3=F64, 4=Heap, 5=Undef). Used by
/// ssa_lower dispatch sites that need to branch on the runtime
/// type of an Any.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64 {
    if is_int32(v) {
        AnySlotTag::I64 as i64
    } else if is_double(v) {
        AnySlotTag::F64 as i64
    } else if is_null(v) {
        AnySlotTag::Null as i64
    } else if is_undefined(v) {
        AnySlotTag::Undef as i64
    } else if is_bool(v) {
        AnySlotTag::Bool as i64
    } else if is_cell(v) {
        AnySlotTag::Heap as i64
    } else {
        // Defensive — `v == 0` (uninitialized slot) treated as Null.
        AnySlotTag::Null as i64
    }
}

/// Decode an [`AnyValue`] to the legacy raw-`i64` value shape
/// the boxed-pair callers consumed:
///
/// - `Int32` → sign-extended to i64
/// - `f64`   → IEEE 754 bits
/// - `Bool`  → 0 / 1
/// - `Null` / `Undef` → 0
/// - `Cell`  → pointer as i64 (top 16 bits zero, low 48 bits VA)
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_anyv_unbox_value(v: AnyValue) -> i64 {
    if is_int32(v) {
        as_int32(v) as i64
    } else if is_double(v) {
        as_double(v).to_bits() as i64
    } else if is_null(v) || is_undefined(v) {
        0
    } else if is_bool(v) {
        if as_bool(v) { 1 } else { 0 }
    } else if is_cell(v) {
        v as i64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{VALUE_FALSE, VALUE_TRUE, box_double, box_int32};

    #[test]
    fn anyv_box_helpers_match_constants() {
        assert_eq!(__torajs_anyv_box_i32(0), box_int32(0));
        assert_eq!(__torajs_anyv_box_i32(-1), box_int32(-1));
        assert_eq!(__torajs_anyv_box_f64(3.14), box_double(3.14));
        assert_eq!(__torajs_anyv_box_bool(0i64), VALUE_FALSE);
        assert_eq!(__torajs_anyv_box_bool(1i64), VALUE_TRUE);
        assert_eq!(__torajs_anyv_box_bool(-1i64), VALUE_TRUE);
        assert_eq!(__torajs_anyv_null(), VALUE_NULL);
        assert_eq!(__torajs_anyv_undefined(), VALUE_UNDEFINED);
    }

    #[test]
    fn anyv_box_pointer_null_yields_null_sentinel() {
        unsafe {
            assert_eq!(__torajs_anyv_box_pointer(std::ptr::null_mut()), VALUE_NULL);
        }
    }

    #[test]
    fn anyv_box_i64_fits_i32() {
        for x in [0i64, 1, -1, 42, i32::MAX as i64, i32::MIN as i64] {
            let v = __torajs_anyv_box_i64(x);
            assert!(is_int32(v), "{x} should round-trip as Int32");
            assert_eq!(as_int32(v) as i64, x);
        }
    }

    #[test]
    fn anyv_box_i64_overflows_to_f64() {
        for x in [i32::MAX as i64 + 1, i32::MIN as i64 - 1, i64::MAX, i64::MIN] {
            let v = __torajs_anyv_box_i64(x);
            assert!(is_double(v), "{x} should promote to f64");
            assert!(!is_int32(v));
        }
    }

    #[test]
    fn anyv_box_from_pair_dispatch() {
        unsafe {
            assert_eq!(__torajs_anyv_box_from_pair(0, 0), VALUE_NULL);
            assert_eq!(__torajs_anyv_box_from_pair(1, 0), VALUE_FALSE);
            assert_eq!(__torajs_anyv_box_from_pair(1, 1), VALUE_TRUE);
            let v = __torajs_anyv_box_from_pair(2, 42);
            assert!(is_int32(v));
            assert_eq!(as_int32(v), 42);
            let v = __torajs_anyv_box_from_pair(3, 3.14f64.to_bits() as i64);
            assert!(is_double(v));
            assert_eq!(as_double(v), 3.14);
            assert_eq!(__torajs_anyv_box_from_pair(4, 0), VALUE_NULL);
            assert_eq!(__torajs_anyv_box_from_pair(5, 0), VALUE_UNDEFINED);
            assert_eq!(__torajs_anyv_box_from_pair(99, 0), VALUE_NULL);
        }
    }

    #[test]
    fn anyv_unbox_tag_for_each_kind() {
        assert_eq!(__torajs_anyv_unbox_tag(VALUE_NULL), AnySlotTag::Null as i64);
        assert_eq!(
            __torajs_anyv_unbox_tag(VALUE_UNDEFINED),
            AnySlotTag::Undef as i64
        );
        assert_eq!(__torajs_anyv_unbox_tag(VALUE_TRUE), AnySlotTag::Bool as i64);
        assert_eq!(
            __torajs_anyv_unbox_tag(VALUE_FALSE),
            AnySlotTag::Bool as i64
        );
        assert_eq!(
            __torajs_anyv_unbox_tag(box_int32(42)),
            AnySlotTag::I64 as i64
        );
        assert_eq!(
            __torajs_anyv_unbox_tag(box_double(3.14)),
            AnySlotTag::F64 as i64
        );
    }

    #[test]
    fn anyv_unbox_value_for_each_kind() {
        assert_eq!(__torajs_anyv_unbox_value(VALUE_NULL), 0);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_UNDEFINED), 0);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_TRUE), 1);
        assert_eq!(__torajs_anyv_unbox_value(VALUE_FALSE), 0);
        assert_eq!(__torajs_anyv_unbox_value(box_int32(42)), 42);
        assert_eq!(__torajs_anyv_unbox_value(box_int32(-7)), -7);
        let bits = __torajs_anyv_unbox_value(box_double(3.14));
        assert_eq!(f64::from_bits(bits as u64), 3.14);
    }

    #[test]
    fn anyv_box_from_pair_round_trip_with_unbox() {
        unsafe {
            for (t, v) in [(0i64, 0i64), (1, 1), (2, 42), (3, 0), (5, 0)] {
                let any = __torajs_anyv_box_from_pair(t, v);
                let t2 = __torajs_anyv_unbox_tag(any);
                let v2 = __torajs_anyv_unbox_value(any);
                if t == 3 {
                    assert_eq!(t2, 3, "F64 tag should round-trip");
                } else {
                    assert_eq!(t2, t, "tag {t} → {t2}");
                }
                assert_eq!(v2, v, "value mismatch for tag {t}");
            }
        }
    }
}
