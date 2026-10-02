//! The match-length lane's read side: which operands are a measured
//! `s.match(re)` result held as its first match's length.
//!
//! The census (`ast::regex_match_len_only`) lists the match calls
//! whose result is only null-tested or asked `m[0].length`;
//! `emit_match` answers those with `__torajs_str_match_len` + 1 when
//! the receiver is a string and the argument a RegExp, so the value is
//! an `I64`: 0 for null, the length plus one for a match. The offset
//! keeps every truthiness consumer right without a change (an `I64`
//! tests `!= 0`, and an empty match is still truthy). Three places
//! answer from that number: the binding (`finalize_and_bind`), the
//! null comparison (`lower_binop_with_ids`) and `m[0].length`
//! (`ssa_lower_member::lower`).

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `eid` is a match call the census listed.
pub(crate) fn is_measured_match(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    matches!(
        ctx.ast.get_expr(eid),
        Expr::Call { callee, .. } if ctx.ast.regex_match_len_callees.contains(callee)
    )
}

/// `e` names a binding that holds a match length: the checker types it
/// an array (`string[]`, or `string[] | null` under an annotation)
/// while its slot holds an `I64`. The split-count lane binds the same
/// pair, and needs no telling apart: a binding gets the integer slot
/// only when every one of its uses is its own lane's, and the two use
/// sets are disjoint — `x.length` there; null tests, truthiness and
/// `m[0].length` here. `ctx.locals` answers the binding in scope, so a
/// shadowing inner declaration keeps its own reading.
fn is_match_len_binding(ctx: &LowerCtx<'_>, e: ExprId) -> bool {
    let Expr::Ident(n) = ctx.ast.get_expr(e) else {
        return false;
    };
    let arr = |t: &crate::check::Type| matches!(t, crate::check::Type::Array(_));
    matches!(
        ctx.expr_types.get(&e),
        Some(t) if arr(t) || matches!(t, crate::check::Type::Nullable(inner) if arr(inner))
    ) && ctx.locals.get(n).is_some_and(|l| l.ty == Type::I64)
}

/// `m === null` / `!==` / `==` / `!=` over a match-length binding and
/// a bare `null`: the slot is 0 exactly when the match answered null.
pub(crate) fn null_test(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: &Operand,
    b: &Operand,
    left: Option<ExprId>,
    right: Option<ExprId>,
) -> Option<Operand> {
    let pred = match op {
        AstBinOp::Eq | AstBinOp::LooseEq => IPred::Eq,
        AstBinOp::Neq | AstBinOp::LooseNeq => IPred::Ne,
        _ => return None,
    };
    let is_null = |e: Option<ExprId>| e.is_some_and(|e| matches!(ctx.ast.get_expr(e), Expr::Null));
    let held = if left.is_some_and(|e| is_match_len_binding(ctx, e)) && is_null(right) {
        *a
    } else if right.is_some_and(|e| is_match_len_binding(ctx, e)) && is_null(left) {
        *b
    } else {
        return None;
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(pred, held, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Some(Operand::Value(v))
}

/// `m[0].length` over a match-length binding: the slot minus one. The
/// index is never lowered — there is no array to read.
pub(crate) fn first_length(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    if name != "length" {
        return None;
    }
    let Expr::Index { obj: m, index } = ctx.ast.get_expr(obj) else {
        return None;
    };
    let (m, index) = (*m, *index);
    if !matches!(ctx.ast.get_expr(index), Expr::Number(n) if *n == 0.0)
        || !is_match_len_binding(ctx, m)
    {
        return None;
    }
    let held = ctx.lower_expr(m);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::Sub, held, Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    Some(Operand::Value(v))
}
