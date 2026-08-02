//! Generator parameter rewrite helpers.
//!
//! Chunk 336 — extracted from `ast::desugar_generators` to shrink
//! the parent fn below the 200-LOC HARD limit. Walks every Expr
//! reachable from a generator body and rewrites every `Ident(name)`
//! matching a generator parameter (or lifted-local) name into
//! `this.<name>` in-place, so the binding survives yield
//! boundaries via the iterator-class field path.

use super::{Ast, Expr, ExprId, Param, Stmt};

/// Walk every Expr reachable from `body` and rewrite param-name
/// idents in-place into `this.<name>` field accesses. The same
/// ExprId keeps its semantic meaning; only its kind changes from
/// `Ident` to `Member { obj: this, name }`.
pub(super) fn rewrite_params_to_this(ast: &mut Ast, body: &[Stmt], params: &[Param]) {
    // Identity map: each name rewrites to a this-field of the SAME
    // name. RFC 20260802 generalized the walker to a name→field map
    // so the catch-param slot rewrite below can share it.
    let map: std::collections::HashMap<String, String> = params
        .iter()
        .map(|p| (p.name.clone(), p.name.clone()))
        .collect();
    let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for s in body {
        rewrite_params_in_stmt(ast, s, &map, &mut visited);
    }
}

/// RFC 20260802 — rewrite every `Ident(from)` reachable from `body`
/// into `this.<slot>`. Used by the generator try-region arm to point
/// catch-param references at the hoisted exception field.
pub(super) fn rewrite_ident_to_this_slot(ast: &mut Ast, body: &[Stmt], from: &str, slot: &str) {
    let mut map = std::collections::HashMap::new();
    map.insert(from.to_string(), slot.to_string());
    let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for s in body {
        rewrite_params_in_stmt(ast, s, &map, &mut visited);
    }
}

fn rewrite_params_in_stmt(
    ast: &mut Ast,
    s: &Stmt,
    pset: &std::collections::HashMap<String, String>,
    visited: &mut std::collections::HashSet<u32>,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            rewrite_params_in_expr(ast, *eid, pset, visited);
        }
        Stmt::YieldInto { value, .. } => {
            rewrite_params_in_expr(ast, *value, pset, visited);
        }
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                rewrite_params_in_expr(ast, *eid, pset, visited);
            }
        }
        Stmt::LetDecl { init, .. } => rewrite_params_in_expr(ast, *init, pset, visited),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_params_in_expr(ast, *cond, pset, visited);
            rewrite_params_in_stmt(ast, then_branch, pset, visited);
            if let Some(e) = else_branch {
                rewrite_params_in_stmt(ast, e, pset, visited);
            }
        }
        Stmt::Labeled { body, .. } => {
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        // F1 (RFC 20260728-gen-forof-yieldstar) — a yield-free for-of
        // kept inline by the state machine still reads lifted outer
        // locals through its element expr and body. (The `src_ident`
        // NAME contract with a lifted source is a separate registered
        // hole — the checker resolves it by name, not ExprId.)
        Stmt::ForOf {
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            rewrite_params_in_expr(ast, *elem_expr, pset, visited);
            if let Some(o) = forin_obj {
                rewrite_params_in_expr(ast, *o, pset, visited);
            }
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_params_in_expr(ast, *parent, pset, visited);
            rewrite_params_in_expr(ast, *sep, pset, visited);
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::While { cond, body } => {
            rewrite_params_in_expr(ast, *cond, pset, visited);
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::DoWhile { body, cond } => {
            rewrite_params_in_stmt(ast, body, pset, visited);
            rewrite_params_in_expr(ast, *cond, pset, visited);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rewrite_params_in_stmt(ast, i, pset, visited);
            }
            if let Some(c) = cond {
                rewrite_params_in_expr(ast, *c, pset, visited);
            }
            if let Some(st) = step {
                rewrite_params_in_expr(ast, *st, pset, visited);
            }
            rewrite_params_in_stmt(ast, body, pset, visited);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_params_in_stmt(ast, s, pset, visited);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_params_in_expr(ast, *scrutinee, pset, visited);
            for c in cases {
                rewrite_params_in_expr(ast, c.value, pset, visited);
                for s in &c.body {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
            if let Some(d) = default {
                for s in d {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
        }
        // RFC 20260802 — descend into EVERY try: a yield-bearing one
        // gets region-lowered, and even a yield-free try kept inline
        // still reads generator params / lifted locals through its
        // body (the same reason inline loops descend — a `this.i`
        // read doesn't care whether a try wraps it). Its own `let`s
        // stay plain locals (lift_lets keeps that gate). The catch
        // param shadows any same-named generator param / lifted
        // local inside the catch body, so it drops out of the
        // rewrite map there.
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_params_in_stmt(ast, s, pset, visited);
            }
            let reduced;
            let catch_set = match catch_param {
                Some(p) if pset.contains_key(p) => {
                    let mut c = pset.clone();
                    c.remove(p);
                    reduced = c;
                    &reduced
                }
                _ => pset,
            };
            for s in catch_body {
                rewrite_params_in_stmt(ast, s, catch_set, visited);
            }
            if let Some(fs) = finally_body {
                for s in fs {
                    rewrite_params_in_stmt(ast, s, pset, visited);
                }
            }
        }
        _ => {}
    }
}

fn rewrite_params_in_expr(
    ast: &mut Ast,
    eid: ExprId,
    pset: &std::collections::HashMap<String, String>,
    visited: &mut std::collections::HashSet<u32>,
) {
    if !visited.insert(eid.0) {
        return;
    }
    let kind = ast.exprs[eid.0 as usize].clone();
    match kind {
        Expr::Ident(name) if pset.contains_key(&name) => {
            let field = pset[&name].clone();
            let this_id = ast.add_expr(Expr::This);
            ast.exprs[eid.0 as usize] = Expr::Member {
                obj: this_id,
                name: field,
            };
        }
        Expr::BinOp { left, right, .. } => {
            rewrite_params_in_expr(ast, left, pset, visited);
            rewrite_params_in_expr(ast, right, pset, visited);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. }
        | Expr::InstanceOf { expr, .. } => {
            rewrite_params_in_expr(ast, expr, pset, visited);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
        }
        Expr::OptIndex { obj, index } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
            rewrite_params_in_expr(ast, index, pset, visited);
        }
        Expr::OptCall { callee, args } => {
            rewrite_params_in_expr(ast, callee, pset, visited);
            for a in args {
                rewrite_params_in_expr(ast, a, pset, visited);
            }
        }
        Expr::Call { callee, args } => {
            rewrite_params_in_expr(ast, callee, pset, visited);
            for a in args {
                rewrite_params_in_expr(ast, a, pset, visited);
            }
        }
        Expr::Assign { target, value } => {
            rewrite_params_in_expr(ast, target, pset, visited);
            rewrite_params_in_expr(ast, value, pset, visited);
        }
        Expr::Index { obj, index } => {
            rewrite_params_in_expr(ast, obj, pset, visited);
            rewrite_params_in_expr(ast, index, pset, visited);
        }
        Expr::Array(els) => {
            for e in els {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_params_in_expr(ast, cond, pset, visited);
            rewrite_params_in_expr(ast, then_branch, pset, visited);
            rewrite_params_in_expr(ast, else_branch, pset, visited);
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_params_in_expr(ast, lhs, pset, visited);
            rewrite_params_in_expr(ast, rhs, pset, visited);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                rewrite_params_in_expr(ast, e, pset, visited);
            }
        }
        Expr::PostIncr { target, .. } => {
            rewrite_params_in_expr(ast, target, pset, visited);
        }
        _ => {}
    }
}
