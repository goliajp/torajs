//! RFC 20260719-fn-tostring-source B4b — `f.toString()` on a
//! statically-typed top-level fn ident folds to the type-erased
//! source text at compile time (the same `erase_types` output the
//! fn-addr registry bakes, so the static and any lanes answer
//! byte-identical strings).
//!
//! Gate mirrors the checker's route_early wedge: callee is
//! `Member { obj, "toString" }` with zero args and either `obj` is
//! an Ident naming a top-level `Stmt::FnDecl` (compile-time fold —
//! a sentinel-span decl folds the JSC native form) or the checker
//! typed `obj` as `Function` (B6a — closure bindings / fn params /
//! fn-typed fields route the value through the runtime erased-source
//! kernels by SSA repr). The B6a leg keys on `ctx.expr_types` — the
//! same checker truth the route_early wedge reads — so the admitted
//! and lowered shapes can't drift apart.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260719-fn-tostring-source B6 — a member read on a builtin
/// namespace stand-in (`Math.max` / `console.log` / `JSON.stringify`:
/// the object types as `Type::Object` and the member as `Function`)
/// has no runtime value form — namespace statics lower at their call
/// sites. Every string-consuming face (`.toString()` / `String()` /
/// template substitution / `+` concat) folds the JSC named native
/// form at compile time instead of lowering the value. Returns the
/// text; callers intern. Both gates read `ctx.expr_types` — the
/// checker's own truth — so a user binding shadowing `Math` (typed
/// as anything but `Object`) never folds.
pub(crate) fn namespace_static_native_form(ctx: &LowerCtx<'_>, eid: ExprId) -> Option<String> {
    let Expr::Member {
        obj: ns,
        name: method,
    } = ctx.ast.get_expr(eid)
    else {
        return None;
    };
    if !matches!(ctx.expr_types.get(ns), Some(crate::check::Type::Object(_))) {
        return None;
    }
    if !matches!(
        ctx.expr_types.get(&eid),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    Some(format!("function {method}() {{ [native code] }}"))
}

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member { obj, name } = ctx.ast.get_expr(callee) else {
        return None;
    };
    // §20.2.3.5 — toLocaleString folds toString's answer.
    if (name != "toString" && name != "toLocaleString") || !args.is_empty() {
        return None;
    }
    let obj = *obj;
    if let Expr::Ident(fn_name_ref) = ctx.ast.get_expr(obj) {
        let fn_name_ref = fn_name_ref.clone();
        if let Some((decl_name, span)) = ctx.ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl { name: n, span, .. } if *n == fn_name_ref => Some((n.clone(), *span)),
            _ => None,
        }) {
            let text = if span.start == 0 && span.end == 0 {
                let visible = crate::ssa_lower_inner::strip_static_method_name(
                    &decl_name,
                    &ctx.ast.class_parents,
                )
                .unwrap_or(&decl_name);
                format!("function {visible}() {{ [native code] }}")
            } else {
                crate::fn_source_erase::erase_types(&ctx.ast.source, &ctx.ast.type_ann_spans, span)
            };
            let s = ctx.intern_string_literal(&text);
            return Some(Operand::Value(s));
        }
    }
    // B6 — namespace-static builtin method (`Math.max.toString()`):
    // no value form to hand the runtime kernels; fold the JSC named
    // native form here, before the value leg tries to lower it.
    if let Some(text) = namespace_static_native_form(ctx, obj) {
        let s = ctx.intern_string_literal(&text);
        return Some(Operand::Value(s));
    }
    // B6a — fn-typed VALUE receiver: runtime kernel by SSA repr.
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Function(..))
    ) {
        return None;
    }
    let op = ctx.lower_expr(obj);
    let arg_ty = ctx.operand_ty(&op);
    match arg_ty {
        Type::FnSig(_) => Some(Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.fn_source_str, vec![op]),
            Type::Str,
            None,
        ))),
        Type::Closure(_) => {
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.closure_source_str, vec![op.clone()]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(obj, &op);
            Some(Operand::Value(s))
        }
        other => panic!("ssa-lower: fn toString on Function-typed receiver with repr {other:?}"),
    }
}
