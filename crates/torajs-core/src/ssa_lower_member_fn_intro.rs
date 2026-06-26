//! T-27.c — `f.length` / `f.name` for FnDecl Ident or
//! closure-local-binding pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Member` god-arm
//! as chunk-61 of the decomp (chunks 1-60 = ... + Bun.file.size +
//! Response.status web/runtime cluster).
//!
//! Two paths share the same `Member{Ident, "length"|"name"}` gate:
//!
//! - **Path 1 — top-level FnDecl**: detected early because the
//!   regular `lower_expr(obj)` panics on unknown ident for a
//!   top-level fn (which doesn't appear in `locals`; for generic
//!   fns it's not in `fn_table` either — only mono-named entries
//!   are). Resolve by walking `ast.stmts` to find the matching
//!   `Stmt::FnDecl`. Compile-time fold the param count / name from
//!   the AST.
//! - **Path 2 — closure local-binding**: `let f = function() {}`
//!   where `f` is in `locals` as `Type::Closure`. Same fold using
//!   its `SigId` from `fn_sig_ids`.
//!
//! Synthetic-param filtering for `.length`:
//! - `__env` (lifted closure env ptr)
//! - `__this` (class method receiver)
//! - `__torajs_real_argc` (T-31 hidden arg-count prefix)
//! - rest-params (`...args`) per ES spec `function.length`
//!
//! Synthetic-name filtering for `.name`:
//! - `__closure_N` lifted-closure synthetic name → `""` per JS
//!   spec (anonymous fn expressions return empty `.name` unless
//!   assigned to a tracked binder).
//!
//! Returns `Some(op)` on hit; `None` on miss (obj not Ident, name
//! not `length`/`name`, or neither path resolves the ident).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    if name != "length" && name != "name" {
        return None;
    }
    let Expr::Ident(fn_name_ref) = ctx.ast.get_expr(obj) else {
        return None;
    };
    let fn_name_ref = fn_name_ref.clone();
    if let Some(op) = try_top_level_fn_decl(ctx, &fn_name_ref, name) {
        return Some(op);
    }
    try_closure_local_binding(ctx, &fn_name_ref, name)
}

fn try_top_level_fn_decl(ctx: &mut LowerCtx<'_>, fn_name_ref: &str, name: &str) -> Option<Operand> {
    let fn_decl = ctx.ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n, params, ..
        } if n == fn_name_ref => Some((n.clone(), params.clone())),
        _ => None,
    })?;
    let (fn_name_owned, params) = fn_decl;
    if name == "length" {
        let visible = params
            .iter()
            .filter(|p| {
                p.name != "__env"
                    && p.name != "__this"
                    && p.name != "__torajs_real_argc"
                    && !p.is_rest
            })
            .count();
        return Some(Operand::ConstI64(visible as i64));
    }
    let visible_name = if fn_name_owned.starts_with("__closure_") {
        String::new()
    } else {
        fn_name_owned
    };
    let s = ctx.intern_string_literal(&visible_name);
    Some(Operand::Value(s))
}

fn try_closure_local_binding(
    ctx: &mut LowerCtx<'_>,
    fn_name_ref: &str,
    name: &str,
) -> Option<Operand> {
    let fid = *ctx.fn_table.get(fn_name_ref)?;
    let sig_id = *ctx.fn_sig_ids.get(&fid)?;
    if name == "length" {
        let (params, _) = &ctx.fn_sigs[sig_id.0 as usize];
        let visible = if !params.is_empty()
            && params[0] == Type::Ptr
            && fn_name_ref.starts_with("__closure_")
        {
            params.len() - 1
        } else {
            params.len()
        };
        return Some(Operand::ConstI64(visible as i64));
    }
    let visible_name = if fn_name_ref.starts_with("__closure_") {
        String::new()
    } else {
        fn_name_ref.to_string()
    };
    let s = ctx.intern_string_literal(&visible_name);
    Some(Operand::Value(s))
}
