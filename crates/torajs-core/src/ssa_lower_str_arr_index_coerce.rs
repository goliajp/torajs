//! Per-needle coerce helper for the
//! [`ssa_lower_str_arr_index_search`] inline scan loop.
//!
//! Carved out as the second axis of the planned 3-way second-pass
//! split (needle_coerce / from_normalize / compare_dispatch). Sits
//! alongside [`super::ssa_lower_str_arr_index_eq`] (compare axis).
//!
//! Pre-fix tora passed the raw needle straight into the
//! ICmp / FCmp dispatch, so an `Array<i64>` with an `f64` needle
//! (e.g. NaN, -0.0, fractional) hit an LLVM verify error
//! ("Found FloatValue but expected IntValue"). JS spec §7.2.13
//! StrictEqualityComparison treats both as Number, so the bridging
//! logic mirrors what the rest of tora does for mixed-numeric BinOp.
//!
//! Const-folded short-circuits: an `Array<i64>` with a `ConstF64`
//! needle that is non-finite, NaN, ±∞ or has a non-zero fractional
//! part cannot strictly-equal any element. Caller short-circuits via
//! the [`Err`] arm:
//!
//! - `arr.indexOf` / `arr.lastIndexOf` → `Operand::ConstI64(-1)`
//! - `arr.includes` → `Operand::ConstBool(false)`

use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Bridge the needle's lowered Operand to the receiver's element-type
/// ABI expected by the per-elem-ty compare dispatch.
///
/// Returns:
/// - `Ok(needle)` — coerced needle ready for the scan loop's compare
///   dispatch.
/// - `Err(value)` — a const-folded short-circuit result the caller
///   should return as the method's final value (see module doc).
pub(crate) fn coerce_needle(
    ctx: &mut LowerCtx<'_>,
    needle_raw: Operand,
    needle_ty: Type,
    elem_ty: Type,
    want_bool: bool,
) -> Result<Operand, Operand> {
    match (elem_ty, needle_ty) {
        (a, b) if a == b => Ok(needle_raw),
        (Type::I64, Type::F64) => {
            // Const f64 that's an integer in i64
            // range converts cleanly. Anything
            // else (NaN / Infinity / fractional)
            // can't equal any i64 element, so
            // short-circuit to -1 / false.
            if let Operand::ConstF64(n) = needle_raw
                && n.is_finite()
                && n.fract() == 0.0
                && n >= i64::MIN as f64
                && n <= i64::MAX as f64
            {
                Ok(Operand::ConstI64(n as i64))
            } else if let Operand::ConstF64(_) = needle_raw {
                if want_bool {
                    Err(Operand::ConstBool(false))
                } else {
                    Err(Operand::ConstI64(-1))
                }
            } else {
                Ok(ctx.coerce_to_i64(needle_raw))
            }
        }
        (Type::F64, Type::I64) => Ok(ctx.coerce_to_f64(needle_raw)),
        _ => Ok(needle_raw),
    }
}
