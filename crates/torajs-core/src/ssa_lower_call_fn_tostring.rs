//! RFC 20260719-fn-tostring-source B4b — `f.toString()` on a
//! statically-typed top-level fn ident folds to the type-erased
//! source text at compile time (the same `erase_types` output the
//! fn-addr registry bakes, so the static and any lanes answer
//! byte-identical strings).
//!
//! Gate mirrors the checker's route_early wedge exactly: callee is
//! `Member { Ident(f), "toString" }` with zero args and `f` names a
//! top-level `Stmt::FnDecl`. A sentinel-span decl (synthesized —
//! not spellable from user source) folds to the JSC native form.
//! Closure local bindings reach toString through the any lane
//! (B4a); their static fold is a recorded B6 boundary.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    if name != "toString" || !args.is_empty() {
        return None;
    }
    let Expr::Ident(fn_name_ref) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    let fn_name_ref = fn_name_ref.clone();
    let (decl_name, span) = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name: n, span, .. } if *n == fn_name_ref => Some((n.clone(), *span)),
        _ => None,
    })?;
    let text = if span.start == 0 && span.end == 0 {
        let visible =
            crate::ssa_lower_inner::strip_static_method_name(&decl_name, &ctx.ast.class_parents)
                .unwrap_or(&decl_name);
        format!("function {visible}() {{\n    [native code]\n}}")
    } else {
        crate::fn_source_erase::erase_types(&ctx.ast.source, &ctx.ast.type_ann_spans, span)
    };
    let s = ctx.intern_string_literal(&text);
    Some(Operand::Value(s))
}
