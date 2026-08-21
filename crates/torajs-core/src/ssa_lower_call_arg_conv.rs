//! The one contract for what has to happen to an argument on its way
//! into a parameter slot.
//!
//! Every call lane has to answer the same question — the argument's
//! bits are shaped like `actual`, the callee will read them as
//! `expected`, so what converts? Each lane used to answer it with its
//! own hand-written `match`, and **the eight copies each knew a
//! different subset**: the direct-call helper and the local-closure
//! lane carried both number lanes but not BigInt; the three
//! fn-indirect copies carried the Any directions but not the number
//! ones (two of them carried nothing but boxing); the fn-value lane
//! carried Any and Substr but not the number ones; and three
//! higher-order call stations each patched a single direction back in
//! on their own side.
//!
//! Nothing announced a gap. A missing direction hands the callee bits
//! shaped for the other lane and it reads them raw — an integer
//! arriving at an f64 parameter is a silent wrong number, not a
//! crash. Worse, **widening a parameter is what makes two call sites
//! disagree, so the call that breaks is never the one that caused
//! it**.
//!
//! So the decision lives here, once, and the lanes keep only their own
//! bookkeeping (which genuinely differs — who owns a freshly minted
//! temp, and when it is released, is a per-lane fact).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// What an argument of type `actual` needs to reach an `expected`
/// parameter. [`arg_conv`] is the only thing that produces one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgConv {
    /// The bits already fit the parameter's lane.
    None,
    /// Between the two number lanes. A JS `number` is one f64; I64 is
    /// the narrower shape of the same value, so crossing is a real
    /// conversion rather than a reinterpretation.
    ToF64,
    ToI64,
    /// Into the Any lane — a pure bit-encode.
    BoxAny,
    /// Out of the Any lane into a number lane (the payload says which).
    AnyToNumber(Type),
    AnyToBool,
    /// Out of the Any lane into `Str` / `BigInt`. Both mint a fresh
    /// owned value that the calling lane releases after the call — the
    /// callee's parameter is a `+0` borrow per the SHARE convention.
    AnyToStr,
    AnyToBigInt,
    /// A borrowed `Substr` view materialized into an owned `Str`, also
    /// released by the calling lane after the call.
    SubstrToStr,
}

/// The contract. `expected` is the parameter's type, `actual` the
/// argument's.
///
/// Order matters only where two arms could both match: `Substr` is
/// checked before the Any arms because a `Substr` is not an `Any`, and
/// the Any-out arms are listed before the number-lane arms because an
/// `Any` argument is never also an `I64`.
pub(crate) fn arg_conv(expected: Type, actual: Type) -> ArgConv {
    match (expected, actual) {
        // A view borrows its bytes from another string; a `Str`
        // parameter outlives the view's guarantees.
        (Type::Str, Type::Substr) => ArgConv::SubstrToStr,
        (Type::Any, Type::Any) => ArgConv::None,
        (Type::Any, _) => ArgConv::BoxAny,
        (Type::F64 | Type::I64, Type::Any) => ArgConv::AnyToNumber(expected),
        (Type::Bool, Type::Any) => ArgConv::AnyToBool,
        (Type::Str, Type::Any) => ArgConv::AnyToStr,
        (Type::BigInt, Type::Any) => ArgConv::AnyToBigInt,
        (Type::F64, Type::I64 | Type::Bool) => ArgConv::ToF64,
        (Type::I64, Type::F64 | Type::Bool) => ArgConv::ToI64,
        _ => ArgConv::None,
    }
}

/// Emit whatever the contract says this argument needs, and answer the
/// operand that belongs in `argv`.
///
/// A conversion that mints a fresh owned value is pushed onto `owned`
/// with its type; the calling lane releases those once the call
/// returns, because the callee's parameter is a `+0` borrow per the
/// SHARE convention.
///
/// `arg` is the argument's expression — boxing reads its static type so
/// an `undefined` / `null` literal keeps its own tag instead of being
/// encoded as whatever its lowered operand happens to look like. Lanes
/// holding only operands (the fn-value lane) match on [`arg_conv`]
/// directly instead; their boxing also has to settle refcounts, which
/// is a fact about that lane rather than about the conversion.
pub(crate) fn emit_arg_conv(
    ctx: &mut LowerCtx<'_>,
    expected: Type,
    arg: ExprId,
    op: Operand,
    owned: &mut Vec<(Operand, Type)>,
) -> Operand {
    // RFC 20260710 C1, at the ARGUMENT position — an `undefined`
    // actual for a sentinel-capable formal must arrive as that width's
    // undefined cell, not as the NULL its literal lowers to. The field
    // position has always done this; a call had no such step, so
    // `f(undefined)` on `f(x: string | undefined)` bound null and every
    // `x === undefined` test inside answered false. `str_undef_sentinel_for`
    // answers None for Any (which boxes to ANY_UNDEF on its own) and
    // for the scalar widths, so those lanes are untouched.
    if matches!(
        ctx.expr_types.get(&arg),
        Some(crate::check::Type::Undefined)
    ) && let Some(sentinel) = ctx.str_undef_sentinel_for(expected)
    {
        return sentinel;
    }
    let actual = ctx.operand_ty(&op);
    // A split product meeting an owned-string parameter (rotation 469,
    // plan-state 469-01): the callee decodes owned cells, so a fresh
    // product is materialized in place and a borrowed one is copied
    // out, the copy released by the calling lane after the call.
    if crate::ssa_lower_assign_slot_fit::arr_views_into_owned_slot(ctx, expected, actual) {
        if ctx.expr_transfers_ownership(arg) {
            return crate::ssa_lower_assign_slot_fit::materialize_fresh_views(ctx, expected, op);
        }
        let copy = crate::ssa_lower_assign_slot_fit::copy_views_out(ctx, expected, op);
        owned.push((copy.clone(), expected));
        return copy;
    }
    match arg_conv(expected, actual) {
        ArgConv::None => op,
        ArgConv::ToF64 => ctx.coerce_to_f64(op),
        ArgConv::ToI64 => ctx.coerce_to_i64(op),
        ArgConv::BoxAny => ctx.box_to_any_from_expr(arg, op),
        ArgConv::AnyToNumber(t) => ctx.coerce_any_to_number(op, t),
        ArgConv::AnyToBool => ctx.coerce_to_bool(op),
        ArgConv::AnyToStr => {
            let s = ctx.coerce_to_str(op, Type::Any);
            owned.push((s, Type::Str));
            s
        }
        ArgConv::AnyToBigInt => {
            let b = ctx.coerce_any_to_bigint(op);
            owned.push((b, Type::BigInt));
            b
        }
        ArgConv::SubstrToStr => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![op]),
                Type::Str,
                None,
            );
            owned.push((Operand::Value(v), Type::Str));
            Operand::Value(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_lanes_convert_nothing() {
        for t in [Type::I64, Type::F64, Type::Any, Type::Str, Type::Bool] {
            assert_eq!(arg_conv(t, t), ArgConv::None);
        }
    }

    #[test]
    fn both_number_lanes_cross() {
        assert_eq!(arg_conv(Type::F64, Type::I64), ArgConv::ToF64);
        assert_eq!(arg_conv(Type::I64, Type::F64), ArgConv::ToI64);
        assert_eq!(arg_conv(Type::F64, Type::Bool), ArgConv::ToF64);
        assert_eq!(arg_conv(Type::I64, Type::Bool), ArgConv::ToI64);
    }

    #[test]
    fn any_goes_both_ways() {
        assert_eq!(arg_conv(Type::Any, Type::I64), ArgConv::BoxAny);
        assert_eq!(arg_conv(Type::Any, Type::Str), ArgConv::BoxAny);
        assert_eq!(
            arg_conv(Type::F64, Type::Any),
            ArgConv::AnyToNumber(Type::F64)
        );
        assert_eq!(
            arg_conv(Type::I64, Type::Any),
            ArgConv::AnyToNumber(Type::I64)
        );
        assert_eq!(arg_conv(Type::Bool, Type::Any), ArgConv::AnyToBool);
        assert_eq!(arg_conv(Type::Str, Type::Any), ArgConv::AnyToStr);
        assert_eq!(arg_conv(Type::BigInt, Type::Any), ArgConv::AnyToBigInt);
    }

    #[test]
    fn a_view_materializes_only_for_a_str_param() {
        assert_eq!(arg_conv(Type::Str, Type::Substr), ArgConv::SubstrToStr);
        assert_eq!(arg_conv(Type::Any, Type::Substr), ArgConv::BoxAny);
        assert_eq!(arg_conv(Type::Substr, Type::Substr), ArgConv::None);
    }
}
