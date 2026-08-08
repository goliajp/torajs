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
//! Returns `Some(op)` on the 1-arg `JSON.parse(x)` Member-Ident
//! shape; `None` otherwise (the 2-arg reviver form is blade 3).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
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
    let text = args[0];
    let op = ctx.lower_expr(text);
    let arg = ctx.box_to_any_from_expr(text, op);
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.json_parse_any, vec![arg]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(out))
}
