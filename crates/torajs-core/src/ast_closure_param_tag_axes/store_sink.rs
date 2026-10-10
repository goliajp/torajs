//! Store-sink axis — a fn-typed parameter whose value the function
//! stores into the heap (`xs.push(f)`, `m.set(k, f)`, `o.k = f`,
//! `xs[i] = f`, `[f]`, `{ k: f }`).
//!
//! Every such slot holds a closure cell: a fn-typed array element or
//! object field is closure repr, and an `any` slot boxes only a cell.
//! A bare pointer stored there is later called through a heap header
//! it does not have (SIGSEGV), so the parameter takes the closure repr
//! and its callers hand it a cell: a named function, or a choice among
//! them, wraps at the call site into its canonical forwarder cell.
//! Calls through the parameter and relays into another fn-typed
//! parameter are not stores and leave the direct-dispatch ABI alone.

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use crate::ast_closure_param_tag_member::passthrough_leaves;
use std::collections::{HashMap, HashSet};

/// Seed marks for the fn-typed parameters a function stores, read off
/// that function's own body (a nested declaration is its own scope).
pub(crate) fn store_sink_param_seeds(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> HashSet<(String, usize)> {
    let mut seeds = HashSet::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        push_child_stmts(s, &mut stack);
        let Stmt::FnDecl { name, body, .. } = s else {
            continue;
        };
        let Some(own) = fn_params.get(name) else {
            continue;
        };
        let mut stored: HashSet<&str> = HashSet::new();
        for st in body {
            visit_stmt_exprs(st, &mut |e| stored_names(ast, e, &mut stored));
        }
        through_bindings(ast, body, &mut stored);
        for (i, p) in own {
            if stored.contains(p.as_str()) {
                seeds.insert((name.clone(), *i));
            }
        }
    }
    seeds
}

/// Every name the module hands to a heap slot, scope-blind: a name
/// collision only costs that binding its direct-dispatch lane.
pub(crate) fn heap_stored_names(ast: &Ast) -> HashSet<String> {
    let mut out = HashSet::new();
    for i in 0..ast.exprs.len() {
        own_stored_names(ast, ExprId(i as u32), &mut out);
    }
    through_bindings(ast, &ast.stmts, &mut out);
    out.into_iter().map(str::to_string).collect()
}

/// A stored binding stores what its initializer answers as well:
/// `const k: F = f; xs.push(k)` hands `f` to the slot, and so does
/// `const k = c ? f : g` both operands. Followed to a fixpoint over
/// every binding in `stmts`, nested ones included.
fn through_bindings<'a>(ast: &'a Ast, stmts: &'a [Stmt], out: &mut HashSet<&'a str>) {
    let mut inits: Vec<(&str, ExprId)> = Vec::new();
    let mut stack: Vec<&Stmt> = stmts.iter().collect();
    while let Some(s) = stack.pop() {
        push_child_stmts(s, &mut stack);
        if let Stmt::LetDecl { name, init, .. } = s {
            inits.push((name, *init));
        }
    }
    let mut grew = true;
    while grew {
        grew = false;
        for &(name, init) in &inits {
            if !out.contains(name) {
                continue;
            }
            for leaf in passthrough_leaves(ast, init) {
                if let Expr::Ident(n) = ast.get_expr(leaf) {
                    grew |= out.insert(n);
                }
            }
        }
    }
}

/// The names `e` and its subexpressions hand to a heap slot.
fn stored_names<'a>(ast: &'a Ast, e: ExprId, out: &mut HashSet<&'a str>) {
    own_stored_names(ast, e, out);
    visit_expr_children(ast, e, &mut |c| stored_names(ast, c, out));
}

/// The names `e` itself, not its subexpressions, hands to a heap slot.
fn own_stored_names<'a>(ast: &'a Ast, e: ExprId, out: &mut HashSet<&'a str>) {
    let stored: &[ExprId] = match ast.get_expr(e) {
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Member { name, .. }
                if matches!(name.as_str(), "push" | "unshift" | "set" | "add") =>
            {
                args
            }
            _ => &[],
        },
        Expr::Assign { target, value } => match ast.get_expr(*target) {
            Expr::Member { .. } | Expr::Index { .. } => std::slice::from_ref(value),
            _ => &[],
        },
        Expr::Array(els) => els,
        _ => &[],
    };
    let fields = match ast.get_expr(e) {
        Expr::ObjectLit { fields } => fields.iter().map(|(_, v)| *v).collect(),
        _ => Vec::new(),
    };
    for &v in stored.iter().chain(&fields) {
        for leaf in passthrough_leaves(ast, v) {
            if let Expr::Ident(n) = ast.get_expr(leaf) {
                out.insert(n);
            }
        }
    }
}
