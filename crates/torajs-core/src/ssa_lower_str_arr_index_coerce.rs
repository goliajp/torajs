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

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Bridge the needle's lowered Operand to the receiver's element-type
/// ABI expected by the per-elem-ty compare dispatch.
///
/// Returns:
/// - `Ok((needle, needle_ty))` — needle ready for the scan loop's
///   compare dispatch, together with the type the dispatch should see.
///   The two can differ from the input: a needle from a different
///   comparison family is boxed, so the type comes back `Any`.
/// - `Err(value)` — a const-folded short-circuit result the caller
///   should return as the method's final value (see module doc).
pub(crate) fn coerce_needle(
    ctx: &mut LowerCtx<'_>,
    needle_eid: ExprId,
    needle_raw: Operand,
    needle_ty: Type,
    elem_ty: Type,
    want_bool: bool,
) -> Result<(Operand, Type), Operand> {
    // §7.2.15 strict equality and §7.2.10 SameValueZero both compare
    // by TYPE first: a needle from another family cannot equal any
    // element here, but that is an ANSWER (-1 / false), not a reason
    // to refuse the program — `[1, 2, 3].includes("42")` is false.
    // Boxing hands it to the needle-Any arm, which compares by tag,
    // so there is one rule for "can these be equal" instead of a
    // per-type-pair table maintained here.
    if !same_compare_family(elem_ty, needle_ty) {
        let boxed = ctx.box_to_any_from_expr(needle_eid, needle_raw);
        return Ok((boxed, Type::Any));
    }
    coerce_within_family(ctx, needle_raw, needle_ty, elem_ty, want_bool).map(|op| (op, needle_ty))
}

/// Whether an element and a needle of these two SSA types could ever
/// compare equal — i.e. whether they are the same JS type. Numbers
/// span several SSA spellings and so do strings; `Any` is its own
/// case because it carries its type at runtime.
fn same_compare_family(elem_ty: Type, needle_ty: Type) -> bool {
    let numeric = |t| matches!(t, Type::I64 | Type::I32 | Type::F64);
    let stringy = |t| matches!(t, Type::Str | Type::Substr);
    elem_ty == needle_ty
        || matches!(needle_ty, Type::Any)
        || matches!(elem_ty, Type::Any)
        || (numeric(elem_ty) && numeric(needle_ty))
        || (stringy(elem_ty) && stringy(needle_ty))
}

fn coerce_within_family(
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
        // An `any` needle stays boxed — strict equality is by-tag,
        // not a ToNumber coercion, so the compare axis packs the
        // ELEMENT as the pair instead (`emit_compare`'s needle-Any
        // arm). Coercing here would make `indexOf("2" as any)`
        // match 2.
        (_, Type::Any) => Ok(needle_raw),
        _ => Ok(needle_raw),
    }
}
