//! `Uint8Array.fromBase64(string, options)` and
//! `Uint8Array.fromHex(string)` — §23.2.2.1-2.
//!
//! Statics wedge in the `ArrayBuffer.isView` shape next door: the
//! arguments box to `any` because the kernel's own steps are the ones
//! that decide what they are (§23.2 throws a TypeError for a
//! non-String rather than coercing), and the answer is a fresh
//! `Uint8Array`, which only the any lane carries.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj: ns_id, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    let hex = match name.as_str() {
        "fromBase64" => false,
        "fromHex" => true,
        _ => return None,
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*ns_id) else {
        return None;
    };
    if ns != "Uint8Array" {
        return None;
    }
    // The pair helper lowers two slots and fills a missing one with
    // `undefined`, which is exactly what an absent options bag is.
    // `fromHex` reads only the first.
    let ops = crate::ssa_lower_new::lower_borrowed_any_pair(ctx, args);
    let (a0, a1) = (ops[0].0.clone(), ops[1].0.clone());
    let (f, argv) = if hex {
        (ctx.intrinsics.uint8array_from_hex, vec![a0])
    } else {
        (ctx.intrinsics.uint8array_from_base64, vec![a0, a1])
    };
    let cur_block = ctx.cur_block;
    let v = ctx
        .f
        .append_inst(cur_block, InstKind::Call(f, argv), Type::Any, None);
    crate::ssa_lower_new::release_borrowed_any_pair(ctx, ops);
    // Both kernels record a TypeError for a non-String argument and a
    // SyntaxError for text the alphabet rejects.
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}
