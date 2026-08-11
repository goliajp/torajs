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
//! - rest-params (`...args`) per ES spec `function.length`
//!
//! Synthetic-name filtering for `.name`:
//! - `__closure_N` lifted-closure synthetic name → `""` per JS
//!   spec (anonymous fn expressions return empty `.name` unless
//!   assigned to a tracked binder).
//! - `__genexpr_N` hoisted generator-expression decl → the
//!   NamedEvaluation verdict `hoist_gen_fn_exprs` recorded, which is
//!   `""` for one in no naming position.
//!
//! Returns `Some(op)` on hit; `None` on miss (obj not Ident, name
//! not `length`/`name`, or neither path resolves the ident).

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
    if name != "length" && name != "name" && name != "prototype" {
        return None;
    }
    let Expr::Ident(fn_name_ref) = ctx.ast.get_expr(obj) else {
        return None;
    };
    let fn_name_ref = fn_name_ref.clone();
    if name == "prototype" {
        return try_generator_factory_prototype(ctx, &fn_name_ref);
    }
    if let Some(op) = try_top_level_fn_decl(ctx, &fn_name_ref, name) {
        return Some(op);
    }
    try_closure_local_binding(ctx, &fn_name_ref, name)
}

/// RFC 20260713 blade 5 — `g.prototype` on a generator factory fn.
/// Answers via the runtime tag→proto table (`__torajs_anyv_proto_get`)
/// keyed by the desugared `__Gen_<name>` class's tag — the same source
/// `Object.getPrototypeOf(g())` reads, so identity holds for free.
/// Non-generator fn `.prototype` stays on the fall-through paths
/// (ordinary-fn prototype minting is a recorded RFC 残面).
fn try_generator_factory_prototype(ctx: &mut LowerCtx<'_>, fn_name_ref: &str) -> Option<Operand> {
    let cls = ctx.ast.generator_factory_classes.get(fn_name_ref)?;
    let tag = *ctx.class_name_to_tag.get(cls)?;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.proto_get,
            vec![Operand::ConstI64(tag as i64)],
        ),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}

/// ES §10.2.5 ExpectedArgumentCount — `fn.length` counts params up
/// to (not including) the first one carrying a default or the rest
/// param, after dropping tr's synthetic prefix params. A default the
/// materialize pass rewrote to the `undefined` literal still counts
/// as a default (the param HAD an initializer in source).
pub(crate) fn expected_argument_count(params: &[crate::ast::Param]) -> usize {
    params
        .iter()
        .filter(|p| {
            p.name != "__env"
                && p.name != "__this"
                // S3.7 — the argv-face pointer slot never counts
                // either. Unreachable-by-construction today (a
                // compile-time `.length` read kills the argv-face
                // binding chain, and the killed body goes loud), but
                // the fold must not depend on that kill staying true.
                && p.name != "__torajs_argv"
        })
        .take_while(|p| p.default.is_none() && !p.is_rest)
        .count()
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
        return Some(Operand::ConstI64(expected_argument_count(&params) as i64));
    }
    let visible_name = if fn_name_owned.starts_with("__closure_") {
        String::new()
    } else if let Some(n) = ctx.ast.genexpr_names.get(&fn_name_owned) {
        // RFC 20260729-fn-value-any V4 刀 2 — `(function* () {}).name`
        // reads a hoisted generator expression's decl directly (the
        // hoist left an `Ident("__genexpr_N")` in the member's object
        // slot), so this compile-time fold has to answer the same
        // NamedEvaluation verdict the fn-name registry does.
        n.clone()
    } else {
        // ES SetFunctionName — a static-method mangled ident answers
        // the property key (same strip as the fn-addr registry rows).
        crate::ssa_lower_inner::strip_static_method_name(&fn_name_owned, &ctx.ast.class_parents)
            .unwrap_or(&fn_name_owned)
            .to_string()
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
        // Prefer the AST decl — only it knows which params carry
        // defaults (§10.2.5 stops counting at the first one). The
        // binding name (`let f = function (…) {}`) aliases the lifted
        // `__closure_N` decl, so resolve by FnId equality, not name.
        // The sig fallback covers table entries with no AST decl and
        // over-counts defaulted tails (recorded).
        if let Some(ast_params) = ctx.ast.stmts.iter().find_map(|s| match s {
            Stmt::FnDecl {
                name: n, params, ..
            } if n == fn_name_ref || ctx.fn_table.get(n.as_str()) == Some(&fid) => {
                Some(params.clone())
            }
            _ => None,
        }) {
            return Some(Operand::ConstI64(
                expected_argument_count(&ast_params) as i64
            ));
        }
        let (params, _) = &ctx.fn_sigs[sig_id.0 as usize];
        let visible = if !params.is_empty()
            && params[0] == Type::Ptr
            && fn_name_ref.starts_with("__closure_")
        {
            // env + the S1 hidden I64 argc slot (RFC
            // 20260810-indirect-argc-abi).
            params.len() - 2
        } else {
            params.len()
        };
        return Some(Operand::ConstI64(visible as i64));
    }
    let visible_name = if fn_name_ref.starts_with("__closure_") {
        String::new()
    } else {
        crate::ssa_lower_inner::strip_static_method_name(fn_name_ref, &ctx.ast.class_parents)
            .unwrap_or(fn_name_ref)
            .to_string()
    };
    let s = ctx.intern_string_literal(&visible_name);
    Some(Operand::Value(s))
}
