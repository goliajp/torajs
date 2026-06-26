//! Web/runtime Member-access cluster pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-60 of the decomp (chunks 1-59 = ... + Symbol.<wellknown>
//! singleton Member arm).
//!
//! Two atomic Member lowerings:
//!
//! - **T-18.c (v0.5.0)** — `Bun.file(<path>).size`. The receiver
//!   shape is `Call{callee=Bun.file, args=[path]}`. We unwrap the
//!   nested Call, lower the `<path>` arg, dispatch to
//!   `__torajs_fs_size_sync`. Returns `i64` (-1 on missing /
//!   non-regular). The `Bun.file(path)` lowering ordinarily
//!   passthroughs the path Str unchanged, so we re-extract it from
//!   the AST rather than lowering the entire receiver Call (avoids
//!   an extra str dup + drop).
//! - **T-21 (v0.6.0)** — `<response>.status`. Receiver is whatever
//!   the user code chose to bind the awaited fetch to; we identify
//!   it by `check.rs`'s per-Expr type side-channel showing
//!   `Type::Object("Response")`. Status is `i64` at offset 8
//!   (runtime stores the full HTTP code as 8 bytes so the Load
//!   lines up with tora's Number ABI directly — no zext).
//!
//! Returns `Some(op)` on hit; `None` on miss (Member name not
//! `size`/`status`, or receiver shape mismatches).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    match name {
        "size" => try_bun_file_size(ctx, obj),
        "status" => try_response_status(ctx, obj),
        _ => None,
    }
}

fn try_bun_file_size(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Operand> {
    let Expr::Call {
        callee: bf_callee, ..
    } = ctx.ast.get_expr(obj)
    else {
        return None;
    };
    let bf_callee = *bf_callee;
    let Expr::Member {
        obj: ns_id,
        name: m,
    } = ctx.ast.get_expr(bf_callee)
    else {
        return None;
    };
    if m != "file" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Bun" {
        return None;
    }
    let path_eid = if let Expr::Call { args, .. } = ctx.ast.get_expr(obj).clone() {
        args[0]
    } else {
        unreachable!()
    };
    let path_op = ctx.lower_expr(path_eid);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.fs_size_sync, vec![path_op]),
        Type::I64,
        None,
    );
    Some(Operand::Value(v))
}

fn try_response_status(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Option<Operand> {
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Object("Response"))
    ) {
        return None;
    }
    let resp_op = ctx.lower_expr(obj);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(Type::I64, resp_op, 8),
        Type::I64,
        None,
    );
    Some(Operand::Value(v))
}
