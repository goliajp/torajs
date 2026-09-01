//! `__torajs_template_object(site, c0, r0, …)` lowering — the
//! synthetic call `parser/tagged_template.rs` plants for §13.2.8.4
//! GetTemplateObject.
//!
//! Every argument is a compile-time literal: the site number and the
//! (cooked, raw) string pairs. The strings bake as static Str cells
//! (the interned-literal pool) and stream to the runtime kernel as a
//! begin / per-pair / end call sequence — no variadic FFI, no
//! pointer-array marshalling. The kernel caches per site and answers
//! a BORROW of the cached cell: the cache holds the one owning
//! reference and a template object is immortal (frozen, alive for
//! the program), so consumers take no stake and drop nothing.

use crate::ast::{Ast, Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Is `eid` the synthetic GetTemplateObject call? Its answer is a
/// BORROW of the per-site cache (the cache holds the one owning
/// reference; the object is immortal), so no consumer may release
/// it: an argv lane that took it for a fresh call result dropped the
/// cached cell after the first call, and every later evaluation of
/// the same site handed the tag function freed memory (`tag`…`` in
/// a loop through a rest-taking closure printed the substitutions
/// where the strings belonged).
pub(crate) fn is_template_object_call(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Call { callee, .. }
        if matches!(ast.get_expr(*callee), Expr::Ident(n)
            if n == crate::parser::tagged_template::TEMPLATE_OBJECT_CALLEE))
}

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    let site = match ctx.ast.get_expr(*args.first()?) {
        Expr::Number(v) => *v as i64,
        _ => return None,
    };
    let pairs = &args[1..];
    if pairs.len() % 2 != 0 {
        return None;
    }
    let n = (pairs.len() / 2) as i64;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.template_object_begin,
            vec![Operand::ConstI64(site), Operand::ConstI64(n)],
        ),
    );
    for pair in pairs.chunks_exact(2) {
        let (c, r) = (pair[0], pair[1]);
        let Expr::String(cs) = ctx.ast.get_expr(c) else {
            return None;
        };
        let cs = cs.clone();
        let Expr::String(rs) = ctx.ast.get_expr(r) else {
            return None;
        };
        let rs = rs.clone();
        let cv = ctx.intern_string_literal(&cs);
        let rv = ctx.intern_string_literal(&rs);
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.template_object_str,
                vec![Operand::Value(cv), Operand::Value(rv)],
            ),
        );
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.template_object_end, vec![]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
