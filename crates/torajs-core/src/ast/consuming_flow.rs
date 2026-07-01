//! Consuming-parameter static analysis (fixed-point).
//!
//! Chunk 349 — extracted from ast.rs. Walks every FnDecl body iteratively
//! and records, for each top-level function, which parameter positions
//! are "consumed" by the callee (fed into a `__new_*` constructor
//! factory, into another fn whose corresponding param already consumes,
//! or stored into `this.<field>` in a class method). The result is
//! written to `ast.consuming_params` and later read at every Call site
//! by check.rs / ssa_lower to decide whether the caller's non-Copy ident
//! argument should be transferred (moved) or copied.
//!
//! The public entry is `compute_consuming_params`; the two private
//! walkers (`scan_stmt_for_consuming_flow` / `scan_expr_for_consuming_flow`)
//! form one logical unit with it and are only ever called from within
//! the fixed-point loop.

use super::{Ast, Expr, ExprId, Stmt};

/// Static analysis: for each top-level FnDecl, determine which
/// parameter positions get "consumed" by callers. A param consumes
/// if its body reaches one of:
///   - a `__new_*(... <param> ...)` constructor factory call
///   - a call to another fn whose corresponding parameter already
///     consumes
///   - a `this.<field> = <param>` assignment (class method)
///
/// Computes the fixed point: iterate until no fn's bitmap changes.
///
/// Result is written to `ast.consuming_params`. check.rs / ssa_lower
/// query this map at every Call site to decide whether to consume the
/// caller's non-Copy ident arg.
pub fn compute_consuming_params(ast: &mut Ast) {
    use std::collections::HashMap;

    // Snapshot fn signatures (name → param names).
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut fn_bodies: HashMap<String, Vec<Stmt>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            fn_params.insert(
                name.clone(),
                params.iter().map(|p| p.name.clone()).collect(),
            );
            fn_bodies.insert(name.clone(), body.clone());
        }
    }
    if fn_params.is_empty() {
        return;
    }

    // Initial bitmap: all false.
    let mut consuming: HashMap<String, Vec<bool>> = fn_params
        .iter()
        .map(|(n, ps)| (n.clone(), vec![false; ps.len()]))
        .collect();

    // Fixed-point loop. Each round, walk every fn body, see which params
    // flow into a known-consuming sink. Stop when nothing changes.
    let mut changed = true;
    let mut rounds = 0;
    while changed {
        changed = false;
        rounds += 1;
        if rounds > 32 {
            // Safety net: a pathological recursive shape shouldn't
            // explode here. fn_count rounds upper bound suffices in
            // practice (each round monotonically grows the consuming
            // set; cap is fn count + slack).
            break;
        }
        let snapshot = consuming.clone();
        for (fname, params) in &fn_params {
            let body = match fn_bodies.get(fname) {
                Some(b) => b,
                None => continue,
            };
            for s in body {
                scan_stmt_for_consuming_flow(
                    ast,
                    s,
                    fname,
                    params,
                    &snapshot,
                    &mut consuming,
                    &mut changed,
                );
            }
        }
    }

    ast.consuming_params = consuming;
}

/// Walk `s` looking for sites that consume one of `params` (the
/// surrounding fn's parameters). Updates `consuming[fname][i] = true`
/// when found and sets `changed=true`.
fn scan_stmt_for_consuming_flow(
    ast: &Ast,
    s: &Stmt,
    fname: &str,
    params: &[String],
    snapshot: &std::collections::HashMap<String, Vec<bool>>,
    consuming: &mut std::collections::HashMap<String, Vec<bool>>,
    changed: &mut bool,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            scan_expr_for_consuming_flow(ast, *eid, fname, params, snapshot, consuming, changed);
        }
        Stmt::YieldInto { value, .. } => {
            scan_expr_for_consuming_flow(ast, *value, fname, params, snapshot, consuming, changed);
        }
        Stmt::Return(Some(eid)) => {
            scan_expr_for_consuming_flow(ast, *eid, fname, params, snapshot, consuming, changed);
        }
        Stmt::Return(None) => {}
        Stmt::LetDecl { init, .. } => {
            scan_expr_for_consuming_flow(ast, *init, fname, params, snapshot, consuming, changed);
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
            scan_stmt_for_consuming_flow(
                ast,
                then_branch,
                fname,
                params,
                snapshot,
                consuming,
                changed,
            );
            if let Some(eb) = else_branch {
                scan_stmt_for_consuming_flow(ast, eb, fname, params, snapshot, consuming, changed);
            }
        }
        Stmt::While { cond, body } => {
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
        }
        Stmt::DoWhile { body, cond } => {
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_consuming_flow(ast, i, fname, params, snapshot, consuming, changed);
            }
            if let Some(c) = cond {
                scan_expr_for_consuming_flow(ast, *c, fname, params, snapshot, consuming, changed);
            }
            if let Some(st) = step {
                scan_expr_for_consuming_flow(ast, *st, fname, params, snapshot, consuming, changed);
            }
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            scan_expr_for_consuming_flow(
                ast, *scrutinee, fname, params, snapshot, consuming, changed,
            );
            for c in cases {
                scan_expr_for_consuming_flow(
                    ast, c.value, fname, params, snapshot, consuming, changed,
                );
                for s in &c.body {
                    scan_stmt_for_consuming_flow(
                        ast, s, fname, params, snapshot, consuming, changed,
                    );
                }
            }
            if let Some(d) = default {
                for s in d {
                    scan_stmt_for_consuming_flow(
                        ast, s, fname, params, snapshot, consuming, changed,
                    );
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
            for s in catch_body {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    scan_stmt_for_consuming_flow(
                        ast, s, fname, params, snapshot, consuming, changed,
                    );
                }
            }
        }
        _ => {}
    }
}

fn scan_expr_for_consuming_flow(
    ast: &Ast,
    eid: ExprId,
    fname: &str,
    params: &[String],
    snapshot: &std::collections::HashMap<String, Vec<bool>>,
    consuming: &mut std::collections::HashMap<String, Vec<bool>>,
    changed: &mut bool,
) {
    let expr = ast.get_expr(eid).clone();
    match expr {
        Expr::Call { callee, args } => {
            // Decide which arg positions get consumed by this call:
            //  - __new_<C>(...) — every non-Copy arg
            //  - other fns — per snapshot's `consuming` bitmap
            let mut consumes_at: Vec<bool> = vec![false; args.len()];
            if let Expr::Ident(callee_name) = ast.get_expr(callee) {
                if callee_name.starts_with("__new_") {
                    for v in consumes_at.iter_mut() {
                        *v = true;
                    }
                } else if let Some(bm) = snapshot.get(callee_name) {
                    for (i, v) in consumes_at.iter_mut().enumerate() {
                        if let Some(b) = bm.get(i) {
                            *v = *b;
                        }
                    }
                }
            }
            for (i, arg) in args.iter().enumerate() {
                if consumes_at.get(i).copied().unwrap_or(false)
                    && let Expr::Ident(name) = ast.get_expr(*arg)
                    && let Some(idx) = params.iter().position(|p| p == name)
                {
                    let bm = consuming.get_mut(fname).unwrap();
                    if !bm[idx] {
                        bm[idx] = true;
                        *changed = true;
                    }
                }
                scan_expr_for_consuming_flow(
                    ast, *arg, fname, params, snapshot, consuming, changed,
                );
            }
            scan_expr_for_consuming_flow(ast, callee, fname, params, snapshot, consuming, changed);
        }
        Expr::Assign { target, value } => {
            // `this.<field> = <param>` — class-field stores own the
            // value transitively. Detect Member-on-This target shape.
            if let Expr::Member { obj, .. } = ast.get_expr(target)
                && (matches!(ast.get_expr(*obj), Expr::This)
                    || matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "__this"))
                && let Expr::Ident(name) = ast.get_expr(value)
                && let Some(idx) = params.iter().position(|p| p == name)
            {
                let bm = consuming.get_mut(fname).unwrap();
                if !bm[idx] {
                    bm[idx] = true;
                    *changed = true;
                }
            }
            scan_expr_for_consuming_flow(ast, target, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, value, fname, params, snapshot, consuming, changed);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            // Pre-desugar shape: every arg consumed (constructor stores).
            for arg in &args {
                if let Expr::Ident(name) = ast.get_expr(*arg)
                    && let Some(idx) = params.iter().position(|p| p == name)
                {
                    let bm = consuming.get_mut(fname).unwrap();
                    if !bm[idx] {
                        bm[idx] = true;
                        *changed = true;
                    }
                }
                scan_expr_for_consuming_flow(
                    ast, *arg, fname, params, snapshot, consuming, changed,
                );
            }
        }
        Expr::BinOp { left, right, .. } => {
            scan_expr_for_consuming_flow(ast, left, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, right, fname, params, snapshot, consuming, changed);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            scan_expr_for_consuming_flow(ast, expr, fname, params, snapshot, consuming, changed);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            scan_expr_for_consuming_flow(ast, obj, fname, params, snapshot, consuming, changed);
        }
        Expr::Index { obj, index } => {
            scan_expr_for_consuming_flow(ast, obj, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, index, fname, params, snapshot, consuming, changed);
        }
        Expr::Array(els) => {
            for e in els {
                scan_expr_for_consuming_flow(ast, e, fname, params, snapshot, consuming, changed);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                scan_expr_for_consuming_flow(ast, e, fname, params, snapshot, consuming, changed);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_consuming_flow(ast, cond, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(
                ast,
                then_branch,
                fname,
                params,
                snapshot,
                consuming,
                changed,
            );
            scan_expr_for_consuming_flow(
                ast,
                else_branch,
                fname,
                params,
                snapshot,
                consuming,
                changed,
            );
        }
        Expr::Nullish { lhs, rhs } => {
            scan_expr_for_consuming_flow(ast, lhs, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, rhs, fname, params, snapshot, consuming, changed);
        }
        Expr::PostIncr { target, .. } => {
            scan_expr_for_consuming_flow(ast, target, fname, params, snapshot, consuming, changed);
        }
        _ => {}
    }
}
