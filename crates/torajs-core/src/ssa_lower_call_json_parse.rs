//! Expression-position `JSON.parse(text)` — any-lane kernel dispatch
//! (RFC 20260808-json-parse-any blade 2).
//!
//! The caller-typed lane (`ssa_lower_stmt_let_decl_json_parse.rs`)
//! claims `let v: T = JSON.parse(s)` at the statement level and stays
//! the fast path; a call that reaches the Expr::Call dispatch chain
//! has no caller type, so the whole tree parses at runtime into
//! NaN-boxed values (`__torajs_json_parse_any`, torajs-anyvalue
//! `json_any.rs` — ToString(text) + full ECMA-404 grammar +
//! SyntaxError via pending-throw).
//!
//! Returns `Some(op)` on the `JSON.parse(x)` / `JSON.parse(x,
//! reviver)` Member-Ident shapes (blade 3 — a 0-arg call parses
//! `"undefined"`, the runtime SyntaxError; args past slot 2 evaluate
//! for side effects and drop, the S311 stringify shape).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "parse" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "JSON" {
        return None;
    }
    let arg = match args.first() {
        Some(&text) => {
            let op = ctx.lower_expr(text);
            ctx.box_to_any_from_expr(text, op)
        }
        None => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(v)
        }
    };
    let (kernel, argv) = match args.get(1) {
        Some(&rev) => {
            let op = ctx.lower_expr(rev);
            let rev_arg = ctx.box_to_any_from_expr(rev, op);
            (ctx.intrinsics.json_parse_reviver, vec![arg, rev_arg])
        }
        None => (ctx.intrinsics.json_parse_any, vec![arg]),
    };
    for &extra in args.iter().skip(2) {
        let op = ctx.lower_expr(extra);
        let ty = ctx.operand_ty(&op);
        ctx.emit_drop_value(op, ty);
    }
    let out = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(kernel, argv), Type::Any, None);
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
