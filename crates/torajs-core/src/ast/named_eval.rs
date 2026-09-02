//! ES NamedEvaluation binding-name recovery, keyed by the ExprId of
//! the value slot.
//!
//! An anonymous function definition takes its name from the syntactic
//! position it sits in — a declaration's binder (§8.6.2 / §14.3.1.2),
//! an assignment's Ident target (§13.15.2), an object-literal property
//! key (§13.2.5.5). The passes that erase those positions each need
//! the same answer:
//!
//! - `lift_arrow_fns` replaces the arrow with `Expr::Closure` and
//!   pass-2B recovers the name off the closure node (chunks 720 / 724
//!   / 792);
//! - `hoist_gen_fn_exprs` replaces the generator expression with an
//!   `Ident("__genexpr_N")` referring to a synthesized top-level decl,
//!   so by the time pass-2B walks, the position is gone entirely.
//!
//! Both read this one walk instead of keeping private copies. The map
//! is keyed by the ExprId of the value AFTER unwrapping `as` casts, so
//! a caller matches whatever node its own pass put there.
//!
//! Positions NOT covered (same set the closure path has always
//! missed, recorded rather than silently assumed): class-field
//! initializers and `export default`.

use std::collections::HashMap;

use super::{Ast, Expr, ExprId, Stmt};

/// Peel `expr as T` wrappers so the position maps onto the value node
/// itself.
fn unwrap_as(ast: &Ast, eid: ExprId) -> ExprId {
    match ast.get_expr(eid) {
        Expr::As { expr, .. } => unwrap_as(ast, *expr),
        _ => eid,
    }
}

/// `f = () => {}` names the anonymous rhs by the Ident target. A
/// chained `f = g = () => {}` names only the innermost assignment —
/// the outer rhs is an Assign expression, not an anonymous definition.
fn walk_assign(ast: &Ast, eid: ExprId, map: &mut HashMap<ExprId, String>) {
    if let Expr::Assign { target, value } = ast.get_expr(eid) {
        if let Expr::Ident(n) = ast.get_expr(*target) {
            map.insert(unwrap_as(ast, *value), n.clone());
        }
        walk_assign(ast, *value, map);
    }
}

fn walk(ast: &Ast, s: &Stmt, map: &mut HashMap<ExprId, String>) {
    match s {
        Stmt::LetDecl { name, init, .. } => {
            map.insert(unwrap_as(ast, *init), name.clone());
        }
        Stmt::Expr(eid) => walk_assign(ast, *eid, map),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk(ast, then_branch, map);
            if let Some(e) = else_branch {
                walk(ast, e, map);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => walk(ast, body, map),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk(ast, i, map);
            }
            walk(ast, body, map);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for cs in &c.body {
                    walk(ast, cs, map);
                }
            }
            if let Some(d) = default {
                for ds in d {
                    walk(ast, ds, map);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for bs in body.iter().chain(catch_body.iter()) {
                walk(ast, bs, map);
            }
            if let Some(f) = finally_body {
                for fs in f {
                    walk(ast, fs, map);
                }
            }
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for bs in v {
                walk(ast, bs, map);
            }
        }
        Stmt::FnDecl { body, .. } => {
            for bs in body {
                walk(ast, bs, map);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                walk(ast, i, map);
            }
        }
        _ => {}
    }
}

/// Value-slot ExprId → the name that position gives an anonymous
/// function definition sitting in it.
///
/// Every declaration / assignment / property slot lands a row, not
/// only the fn-shaped ones: the caller filters by whatever node its
/// own pass recognises, which keeps this walk free of any one pass's
/// notion of "is a function value".
pub(crate) fn collect_named_eval_positions(ast: &Ast) -> HashMap<ExprId, String> {
    let mut map = HashMap::new();
    for s in &ast.stmts {
        walk(ast, s, &mut map);
    }
    // Chunk 792 — an object-literal field is reachable from call-arg /
    // element / nested-literal positions the statement walk never
    // descends into, so sweep the arena for them. A value node sits in
    // exactly one syntactic position, so the sweep never collides with
    // the walk above.
    for e in &ast.exprs {
        if let Expr::ObjectLit { fields } = e {
            for (fname, val) in fields {
                // 565-03 — a COMPUTED key gives no syntactic name: the
                // field's `__computed_<n>__` spelling is the parser's
                // sentinel, not something the user wrote, and handing
                // it to NamedEvaluation is what made `({ [k]() {} })[k]`
                // print `[Function: __computed_0__]`. §10.2.9 names
                // such a member after its RUNTIME key instead, which
                // only the definition point knows.
                if ast.objlit_computed_keys.contains_key(val) {
                    continue;
                }
                // Function names are `String` (557-02 C group): a key
                // with a lone surrogate leaves the function anonymous.
                if let Some(fname) = fname.as_str() {
                    map.insert(unwrap_as(ast, *val), fname.to_string());
                }
            }
        }
    }
    map
}
