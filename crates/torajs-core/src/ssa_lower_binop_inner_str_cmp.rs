//! String × String equality + ordered comparison BinOp dispatch
//! extracted from [`crate::ssa_lower_binop_inner::lower`]
//! (chunk 188 — continuation of file-size debt cleanup, mirrors
//! chunks 185/186/187 shape).
//!
//! Two cohesive sub-paths, both gated on (a_ty, b_ty) being
//! Str / Substr:
//!
//! - **Equality** (Eq / Neq / LooseEq / LooseNeq) — V3-18 wedge.
//!   Per JS spec §7.2.13 IsLooselyEqual, when both ToPrimitive
//!   are String the compare is content-equal; `str_eq` is the
//!   intrinsic. Substr on either side routes through
//!   `substr_eq_str` (substr always on left); two Substr
//!   operands materialize the lhs to owned Str first then drop
//!   it after the compare (rare path — no current bench /
//!   conformance trigger). Neq / LooseNeq negate the bool
//!   result via `Xor true`.
//!
//! - **Ordered** (Lt / Gt / Le / Ge) — V3-18 m1.h.17.
//!   `str_locale_compare` returns -1/0/1 then ICmp against 0
//!   picks the right predicate (Slt/Sgt/Sle/Sge). Same shape as
//!   the BigInt cmp branch. Substr operands not supported here
//!   yet — would need a substr-vs-str comparator.
//!
//! Returns `Some(Operand)` on hit, `None` when types not both
//! Str/Substr or op outside the handled set; caller falls
//! through to the f64 / i64 fall-through paths below on None.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    // V3-18 wedge — `==` / `!=` on Str/Str also routes to str_eq.
    // Per JS spec §7.2.13 IsLooselyEqual when both ToPrimitive
    // are String, compares as String (which is content-equal).
    // Pre-fix only AstBinOp::Eq / Neq (strict) reached this dispatch;
    // LooseEq / LooseNeq fell through and tora returned false.
    if matches!(
        op,
        AstBinOp::Eq | AstBinOp::Neq | AstBinOp::LooseEq | AstBinOp::LooseNeq
    ) && (a_ty == Type::Str || a_ty == Type::Substr)
        && (b_ty == Type::Str || b_ty == Type::Substr)
    {
        // Pick correct comparator based on operand types. Substr
        // on either side → substr_eq_str (with substr on left).
        let (eq_call, args) = match (a_ty, b_ty) {
            (Type::Str, Type::Str) => (ctx.intrinsics.str_eq, vec![a, b]),
            (Type::Substr, Type::Str) => (ctx.intrinsics.substr_eq_str, vec![a, b]),
            (Type::Str, Type::Substr) => (ctx.intrinsics.substr_eq_str, vec![b, a]),
            (Type::Substr, Type::Substr) => {
                // materialize a to owned, then substr_eq_str(b, a_owned)
                let owned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_to_owned, vec![a]),
                    Type::Str,
                    None,
                );
                let eq = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.substr_eq_str, vec![b, Operand::Value(owned)]),
                    Type::Bool,
                    None,
                );
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.str_drop, vec![Operand::Value(owned)]),
                );
                if matches!(op, AstBinOp::Neq | AstBinOp::LooseNeq) {
                    let r = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Xor,
                            Operand::Value(eq),
                            Operand::ConstBool(true),
                        ),
                        Type::Bool,
                        None,
                    );
                    return Some(Operand::Value(r));
                }
                return Some(Operand::Value(eq));
            }
            _ => unreachable!(),
        };
        let eq_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(eq_call, args),
            Type::Bool,
            None,
        );
        if matches!(op, AstBinOp::Neq | AstBinOp::LooseNeq) {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(
                    SsaBinOp::Xor,
                    Operand::Value(eq_v),
                    Operand::ConstBool(true),
                ),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(r));
        }
        return Some(Operand::Value(eq_v));
    }

    // V3-18 m1.h.17 — Lt/Gt/Le/Ge on two Str operands routes through
    // __torajs_str_locale_compare (returns -1/0/1) then ICmp against 0
    // with the right predicate. Same shape as the BigInt cmp branch.
    // Substr operands not yet supported here — would need a
    // substr-vs-str comparator; can materialize on the fly when those
    // call sites surface in conformance / test262.
    if matches!(
        op,
        AstBinOp::Lt | AstBinOp::Gt | AstBinOp::Le | AstBinOp::Ge
    ) && a_ty == Type::Str
        && b_ty == Type::Str
    {
        let c = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_locale_compare, vec![a, b]),
            Type::I64,
            None,
        );
        let pred = match op {
            AstBinOp::Lt => IPred::Slt,
            AstBinOp::Gt => IPred::Sgt,
            AstBinOp::Le => IPred::Sle,
            AstBinOp::Ge => IPred::Sge,
            _ => unreachable!(),
        };
        let r = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(pred, Operand::Value(c), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(r));
    }
    None
}
