//! Shadow-aware `Ident(C) → Ident(__class_C)` rewrite for
//! `class_globals` — the value-position class-reference rewrite must
//! respect lexical scope.
//!
//! The original rewrite was a flat arena scan: EVERY `Ident` spelling
//! a known class name became the class-object sentinel, including
//! references a local binding actually owns. `class R {} function
//! helper(R: any) { return R + 1 }` silently added 1 to the CLASS
//! (its toString, under `+`), not to the parameter — the
//! param-shadow-class bug (rotation 428 discovery).
//!
//! This walk carries the set of class names still visible at each
//! point. Entering any function scope (statement `FnDecl`, arena
//! `ArrowFn` in all its identities — arrow, fn-expr, generator,
//! method) removes the names its parameters or its OWN scope's
//! declarations rebind; a `catch (R)` removes the name for the catch
//! body. Scope declarations are collected to the function boundary
//! (a nested fn's `var` belongs to the nested fn), at fn-level
//! granularity: a deep block-scoped `let R` hides `R` for the whole
//! enclosing fn body, which can only turn a would-be-rewritten
//! reference into a LOUD unknown identifier — the same precision as
//! `rebinds_in_stmt`, never a silent wrong.
//!
//! `new C()` carries its class in a `String` field (`Expr::New`), not
//! an `Ident` — the deconflict census owns that spelling; this walk
//! only serves value-position reads.

use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// Rewrite every value-position class reference the given scope can
/// still see. `class_set` is the full set of known class names.
pub(super) fn rewrite_class_value_refs(ast: &mut Ast, class_set: &HashSet<String>) {
    let active: HashSet<String> = class_set.clone();
    let stmts = std::mem::take(&mut ast.stmts);
    for s in &stmts {
        rewrite_stmt(ast, s, &active);
    }
    ast.stmts = stmts;
}

/// The names a function scope rebinds: its parameters plus every
/// declaration in its own var scope (any block depth, stopping at
/// nested function boundaries — their declarations are their own).
fn shrunk(active: &HashSet<String>, params: &[Param], body: &[Stmt]) -> HashSet<String> {
    let mut declared: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
    collect_scope_decls(body, &mut declared);
    active.difference(&declared).cloned().collect()
}

fn collect_scope_decls(stmts: &[Stmt], out: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_scope_decls(std::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch.as_deref() {
                    collect_scope_decls(std::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                collect_scope_decls(std::slice::from_ref(body), out)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init.as_deref() {
                    collect_scope_decls(std::slice::from_ref(i), out);
                }
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOf { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::ForOfSplitIter { var_name, body, .. } => {
                out.insert(var_name.clone());
                collect_scope_decls(std::slice::from_ref(body), out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_scope_decls(body, out);
                collect_scope_decls(catch_body, out);
                if let Some(fb) = finally_body {
                    collect_scope_decls(fb, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_scope_decls(&c.body, out);
                }
                if let Some(d) = default {
                    collect_scope_decls(d, out);
                }
            }
            Stmt::Block(b) | Stmt::Multi(b) => collect_scope_decls(b, out),
            _ => {}
        }
    }
}

fn rewrite_stmts(ast: &mut Ast, stmts: &[Stmt], active: &HashSet<String>) {
    for s in stmts {
        rewrite_stmt(ast, s, active);
    }
}

fn rewrite_stmt(ast: &mut Ast, s: &Stmt, active: &HashSet<String>) {
    if active.is_empty() {
        return;
    }
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Throw(eid) => {
            rewrite_expr(ast, *eid, active)
        }
        Stmt::LetDecl { init, .. } => rewrite_expr(ast, *init, active),
        Stmt::FnDecl { params, body, .. } => {
            let inner = shrunk(active, params, body);
            for p in params {
                if let Some(d) = p.default {
                    rewrite_expr(ast, d, &inner);
                }
            }
            rewrite_stmts(ast, body, &inner);
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                let inner = shrunk(active, &c.params, &c.body);
                rewrite_stmts(ast, &c.body, &inner);
            }
            for m in methods.iter().chain(static_methods) {
                let inner = shrunk(active, &m.params, &m.body);
                rewrite_stmts(ast, &m.body, &inner);
            }
            for si in static_init {
                if let super::StaticInit::Block(b) = si {
                    rewrite_stmts(ast, b, active);
                }
            }
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(ast, *cond, active);
            rewrite_stmt(ast, then_branch, active);
            if let Some(eb) = else_branch.as_deref() {
                rewrite_stmt(ast, eb, active);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_expr(ast, *cond, active);
            rewrite_stmt(ast, body, active);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref() {
                rewrite_stmt(ast, i, active);
            }
            if let Some(c) = cond {
                rewrite_expr(ast, *c, active);
            }
            if let Some(st) = step {
                rewrite_expr(ast, *st, active);
            }
            rewrite_stmt(ast, body, active);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_expr(ast, *elem_expr, active);
            rewrite_stmt(ast, body, active);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_expr(ast, *parent, active);
            rewrite_expr(ast, *sep, active);
            rewrite_stmt(ast, body, active);
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(ast, body, active),
        Stmt::Block(b) | Stmt::Multi(b) => rewrite_stmts(ast, b, active),
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_stmts(ast, body, active);
            // catch (R) shadows R for exactly the catch body
            let catch_active: HashSet<String> = match catch_param {
                Some(p) => active.iter().filter(|n| *n != p).cloned().collect(),
                None => active.clone(),
            };
            rewrite_stmts(ast, catch_body, &catch_active);
            if let Some(fb) = finally_body {
                rewrite_stmts(ast, fb, active);
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_expr(ast, *scrutinee, active);
            for c in cases {
                rewrite_expr(ast, c.value, active);
                rewrite_stmts(ast, &c.body, active);
            }
            if let Some(d) = default {
                rewrite_stmts(ast, d, active);
            }
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => rewrite_expr(ast, *eid, active),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner.as_deref() {
                rewrite_stmt(ast, i, active);
            }
        }
        _ => {}
    }
}

fn rewrite_expr(ast: &mut Ast, eid: ExprId, active: &HashSet<String>) {
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Expr::Ident(n) = &ast.exprs[id.0 as usize] {
            if active.contains(n.as_str()) {
                let new_name = format!("__class_{n}");
                ast.exprs[id.0 as usize] = Expr::Ident(new_name);
            }
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::Sequence { left, right }
            | Expr::Assign {
                target: left,
                value: right,
            }
            | Expr::Index {
                obj: left,
                index: right,
            }
            | Expr::OptIndex {
                obj: left,
                index: right,
            }
            | Expr::InstanceOf {
                expr: left,
                rhs: right,
            } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::Delete { expr }
            | Expr::As { expr, .. }
            | Expr::PostIncr { target: expr, .. } => stack.push(expr),
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => stack.push(obj),
            Expr::Call { callee, args }
            | Expr::OptCall { callee, args }
            | Expr::NewDynamic { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::ArrowFn { params, body, .. } => {
                let inner = shrunk(active, &params, &body);
                for p in &params {
                    if let Some(d) = p.default {
                        rewrite_expr(ast, d, &inner);
                    }
                }
                if !inner.is_empty() {
                    rewrite_stmts(ast, &body, &inner);
                }
            }
            _ => {}
        }
    }
}
