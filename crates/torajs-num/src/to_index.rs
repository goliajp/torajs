//! ES §7.1.22 `ToIndex`, over the Number the step actually reads.
//!
//! `ToIndex` is the coercion behind an "index-like" builtin parameter
//! — `BigInt.asIntN`'s `bits`, an `ArrayBuffer` byte offset, a typed
//! array length. Its content is a range test:
//!
//! ```text
//! 1. integer = ToIntegerOrInfinity(value)   // NaN → 0, ±∞ stay ±∞
//! 2. if integer is not in [0, 2^53 - 1] → RangeError
//! 3. return integer
//! ```
//!
//! Both halves of step 1 are invisible to an i64 parameter: NaN and
//! ±∞ have to arrive as themselves for step 2 to have anything to
//! test. Truncating on the way in turns `asIntN(Infinity, 0n)` into
//! `i64::MAX` (or, through `FpToSi`, into poison) and
//! `asIntN(NaN, 255n)` into a request for NaN bits that quietly
//! became something else.
//!
//! On the throw path the return value is not meaningful — the
//! caller's `emit_throw_check` makes it unreachable — but it is 0
//! rather than a poison value so a caller that forgets is merely
//! wrong and not undefined.

unsafe extern "C" {
    /// Resolved at link time to torajs-throw's record-pending-throw
    /// helper; the SSA caller's `emit_throw_check` turns the pending
    /// throw into a user-visible `RangeError`.
    fn __torajs_throw_range_error(msg: *const u8);
}

/// The largest integer JS can represent exactly, and `ToIndex`'s
/// upper bound (§7.1.22 step 2 rejects anything above it).
const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

/// The whole of §7.1.22 except the throw: `Some(index)` when the
/// value is an acceptable index, `None` when step 2 rejects it.
///
/// Split out from the FFI entry point so it can be tested — calling
/// the entry point on a rejecting input aborts under the unit-test
/// runner, which links torajs-throw's real recorder with no handler
/// above it. The throw path's user-visible behaviour is covered by
/// the `bigint-asintn-toindex-001` conformance fixture instead.
pub fn to_index_checked(n: f64) -> Option<i64> {
    // ToIntegerOrInfinity(NaN) is +0, and +0 is in range — so NaN is
    // accepted and answers 0, while ±∞ (which survive the fold) are
    // both out of range.
    if n.is_nan() {
        return Some(0);
    }
    let integer = n.trunc();
    if !(0.0..=MAX_SAFE_INTEGER).contains(&integer) {
        return None;
    }
    Some(integer as i64)
}

/// # Safety
///
/// Trivially safe; `unsafe extern "C"` only for the FFI boundary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_to_index(n: f64) -> i64 {
    match to_index_checked(n) {
        Some(i) => i,
        None => {
            unsafe {
                __torajs_throw_range_error(b"Invalid index\0".as_ptr());
            }
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_in_range_and_folds_toward_zero() {
        assert_eq!(to_index_checked(0.0), Some(0));
        assert_eq!(to_index_checked(3.9), Some(3));
        assert_eq!(to_index_checked(-0.0), Some(0));
        assert_eq!(to_index_checked(f64::NAN), Some(0));
        assert_eq!(to_index_checked(MAX_SAFE_INTEGER), Some(9007199254740991));
    }

    #[test]
    fn rejects_out_of_range() {
        assert_eq!(to_index_checked(-1.0), None);
        assert_eq!(to_index_checked(-2.5), None);
        assert_eq!(to_index_checked(f64::INFINITY), None);
        assert_eq!(to_index_checked(f64::NEG_INFINITY), None);
        assert_eq!(to_index_checked(MAX_SAFE_INTEGER + 1.0), None);
    }
}
