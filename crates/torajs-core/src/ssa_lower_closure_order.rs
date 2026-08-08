//! Pass 2B closure-body lowering order — construction before body.
//!
//! A lifted closure's body lowering reads `closure_captures`, which its
//! CONSTRUCTION site (an `Expr::Closure` node in the enclosing fn's
//! body) populates. M2.A ordered the bodies by reverse append order,
//! which equals parent-before-child only while the arena holds inner
//! expressions before outer ones — true for parser output, broken by
//! `desugar_capturing_nested_fns`: a nested `function` declaration it
//! routes becomes a function expression minted at the arena TAIL, so
//! the child closure appends after its parent and reverse order lowers
//! the child's body first.
//!
//! This pass derives the real dependency from the AST instead: each
//! closure has exactly one construction site, so "whose body holds my
//! construction" is a parent function — the relation is a forest, no
//! cycles. A stable topological pass over the reverse-append order
//! keeps today's order wherever it already satisfies parent-first
//! (parser-shaped nesting is untouched) and defers only a child that
//! appears ahead of its parent.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa::FuncId;
use std::collections::{HashMap, HashSet};

/// Reorder `closure_decls` (already reverse-append) so every closure
/// lowers after the closure whose body holds its construction site.
/// Construction sites in user fns or implicit main need nothing: those
/// lower in Pass 2A / Pass 3, before every closure body.
pub(crate) fn order_closure_bodies(
    ast: &Ast,
    closure_decls: Vec<(usize, FuncId)>,
) -> Vec<(usize, FuncId)> {
    // parent[name] = closure whose body constructs `name`.
    let mut parent: HashMap<String, String> = HashMap::new();
    for (stmt_idx, _) in &closure_decls {
        let Stmt::FnDecl { name, body, .. } = &ast.stmts[*stmt_idx] else {
            continue;
        };
        let mut constructed: Vec<String> = Vec::new();
        for s in body {
            closures_in_stmt(ast, s, &mut constructed);
        }
        for c in constructed {
            parent.insert(c, name.clone());
        }
    }
    let names: Vec<Option<&str>> = closure_decls
        .iter()
        .map(|(i, _)| match &ast.stmts[*i] {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    // Stable Kahn over the incoming order: emit the first entry whose
    // parent is already emitted (or is not a closure). The relation is
    // a forest, so this always drains.
    let mut emitted: HashSet<String> = HashSet::new();
    let mut done: Vec<bool> = vec![false; closure_decls.len()];
    let mut out: Vec<(usize, FuncId)> = Vec::with_capacity(closure_decls.len());
    while out.len() < closure_decls.len() {
        let mut progressed = false;
        for k in 0..closure_decls.len() {
            if done[k] {
                continue;
            }
            let ready = match names[k].and_then(|n| parent.get(n)) {
                Some(p) => emitted.contains(p),
                None => true,
            };
            if ready {
                done[k] = true;
                if let Some(n) = names[k] {
                    emitted.insert(n.to_string());
                }
                out.push(closure_decls[k]);
                progressed = true;
            }
        }
        if !progressed {
            // A parent recorded in the map but absent from this decl
            // set (can't lower here) — emit the remainder in incoming
            // order rather than loop forever.
            for k in 0..closure_decls.len() {
                if !done[k] {
                    done[k] = true;
                    out.push(closure_decls[k]);
                }
            }
        }
    }
    out
}

/// Collect the names of every `Expr::Closure` construction site inside
/// `s`, including sites buried in nested expressions.
fn closures_in_stmt(ast: &Ast, s: &Stmt, out: &mut Vec<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) | Stmt::Throw(eid) => {
            closures_in_expr(ast, *eid, out)
        }
        Stmt::YieldInto { value, .. } => closures_in_expr(ast, *value, out),
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            closures_in_expr(ast, *init, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            closures_in_expr(ast, *cond, out);
            closures_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                closures_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            closures_in_expr(ast, *cond, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            closures_in_expr(ast, *scrutinee, out);
            for c in cases {
                closures_in_expr(ast, c.value, out);
                for s in &c.body {
                    closures_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    closures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                closures_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                closures_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                closures_in_expr(ast, *st, out);
            }
            closures_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                closures_in_stmt(ast, st, out);
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            closures_in_expr(ast, *parent, out);
            closures_in_expr(ast, *sep, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            closures_in_expr(ast, *elem_expr, out);
            closures_in_stmt(ast, body, out);
        }
        Stmt::Labeled { body, .. } => closures_in_stmt(ast, body, out),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
            for s in catch_body {
                closures_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    closures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::FnDecl { body, .. } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                closures_in_stmt(ast, inner, out);
            }
        }
        Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } | Stmt::ImportDecl { .. } => {}
    }
}

fn closures_in_expr(ast: &Ast, eid: ExprId, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Closure { fn_name, .. } => out.push(fn_name.clone()),
        Expr::ArrowFn { body, .. } => {
            for s in body {
                closures_in_stmt(ast, s, out);
            }
        }
        Expr::Elision
        | Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget => {}
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            closures_in_expr(ast, *left, out);
            closures_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => closures_in_expr(ast, *expr, out),
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => closures_in_expr(ast, *obj, out),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            closures_in_expr(ast, *callee, out);
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::Assign { target, value } => {
            closures_in_expr(ast, *target, out);
            closures_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            closures_in_expr(ast, *obj, out);
            closures_in_expr(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                closures_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                closures_in_expr(ast, *e, out);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            closures_in_expr(ast, *callee, out);
            for a in args {
                closures_in_expr(ast, *a, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            closures_in_expr(ast, *cond, out);
            closures_in_expr(ast, *then_branch, out);
            closures_in_expr(ast, *else_branch, out);
        }
        Expr::PostIncr { target, .. } => closures_in_expr(ast, *target, out),
    }
}
