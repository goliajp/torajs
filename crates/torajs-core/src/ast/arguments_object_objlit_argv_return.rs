//! The return-escape arm of [`super::arguments_object_objlit_argv`]
//! (r454), scoped per owning function (642-08).
//!
//! `const f = function () { …arguments… }; return f;` hands the caller
//! an opaque closure VALUE, so every call reaches the body through the
//! closure cell's boxed adapter — the same argument the field and
//! store arms make. The alias form needs the binding's ONLY reference
//! to be the return operand. r454 counted references BY NAME over the
//! whole program: two functions each declaring their own `f` made two
//! references, and neither body was admitted — `arguments` inside both
//! closures stayed a bare name ("unknown identifier `arguments`", then
//! a ReferenceError at run time; probe q9/k), while the same program
//! with the locals named `f` and `g` was fine. A local's uses are the
//! uses INSIDE the function that declares it, so the count is taken
//! over the owner's own expression mask — a lifted closure in that
//! body listing the name among its captures counts as a use, since
//! its body is a top-level FnDecl outside the mask — and a name the
//! owner does not declare keeps the program-wide count (a module-top
//! binding returned from a function).

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, ExprId, Stmt};

/// Admit every closure a function returns as a value: a closure
/// literal directly, or a binding whose only reference is the return
/// operand and whose one declaration initializes it with a closure
/// literal.
pub(super) fn admit_return_escapes(
    ast: &Ast,
    excluded: &HashSet<String>,
    ident_refs: &HashMap<&str, usize>,
    closure_refs: &HashMap<&str, usize>,
    value_fns: &mut HashSet<String>,
    method_fns: &mut HashSet<String>,
) {
    let mut all_lets: Vec<(&str, ExprId)> = Vec::new();
    super::arguments_object_chain::collect_letdecls_recursive(&ast.stmts, &mut all_lets);
    for s in &ast.stmts {
        let Stmt::FnDecl { body, .. } = s else {
            continue;
        };
        let mut returns: Vec<ExprId> = Vec::new();
        collect_return_operands(body, &mut returns);
        if returns.is_empty() {
            continue;
        }
        let mut mask = vec![false; ast.exprs.len()];
        super::desugar_eval::body_owned_exprs(ast, body, &mut mask);
        let mut body_lets: Vec<(&str, ExprId)> = Vec::new();
        super::arguments_object_chain::collect_letdecls_recursive(body, &mut body_lets);
        for r in returns {
            let fn_name = match ast.get_expr(r) {
                Expr::Closure { fn_name, .. } => fn_name.clone(),
                Expr::Ident(b) => {
                    let declared_here = body_lets.iter().any(|(n, _)| *n == b.as_str());
                    let (lets, refs) = if declared_here {
                        (&body_lets, uses_in_mask(ast, &mask, b))
                    } else {
                        (&all_lets, ident_refs.get(b.as_str()).copied().unwrap_or(0))
                    };
                    match sole_closure_init(ast, lets, b, refs) {
                        Some(f) => f,
                        None => continue,
                    }
                }
                _ => continue,
            };
            super::arguments_object_objlit_argv::admit_boxed_only(
                ast,
                &fn_name,
                excluded,
                ident_refs,
                closure_refs,
                value_fns,
                method_fns,
            );
        }
    }
}

/// References to `name` inside one owner's expression mask: bare
/// reads plus lifted closures capturing it (their bodies sit outside
/// the mask, so the capture list is where that use shows).
fn uses_in_mask(ast: &Ast, mask: &[bool], name: &str) -> usize {
    ast.exprs
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            mask[*i]
                && match e {
                    Expr::Ident(n) => n == name,
                    Expr::Closure { captures, .. } => captures.iter().any(|c| c == name),
                    _ => false,
                }
        })
        .count()
}

/// The closure `b` is initialized with — when `b` is referenced
/// exactly once (the return operand) and `lets` declares it exactly
/// once, with a closure literal. A second declaration or a non-closure
/// init leaves the name's meaning open: refuse.
fn sole_closure_init(ast: &Ast, lets: &[(&str, ExprId)], b: &str, refs: usize) -> Option<String> {
    if refs != 1 {
        return None;
    }
    let mut init_fn: Option<String> = None;
    for (n, init) in lets {
        if *n != b {
            continue;
        }
        match (init_fn.is_none(), ast.get_expr(*init)) {
            (true, Expr::Closure { fn_name, .. }) => init_fn = Some(fn_name.clone()),
            _ => return None,
        }
    }
    init_fn
}

/// Every `return <expr>` operand reachable from `body`, nested
/// control flow included. Nested FnDecls are walked too — their
/// returns escape a closure value the same way.
fn collect_return_operands(body: &[Stmt], out: &mut Vec<super::ExprId>) {
    for s in body {
        match s {
            Stmt::Return(Some(e)) => out.push(*e),
            Stmt::FnDecl { body, .. } => collect_return_operands(body, out),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_return_operands(core::slice::from_ref(then_branch), out);
                if let Some(e) = else_branch {
                    collect_return_operands(core::slice::from_ref(e), out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => {
                collect_return_operands(core::slice::from_ref(body), out)
            }
            Stmt::For { body, .. } => collect_return_operands(core::slice::from_ref(body), out),
            Stmt::Block(b) | Stmt::Multi(b) => collect_return_operands(b, out),
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_return_operands(body, out);
                collect_return_operands(catch_body, out);
                if let Some(f) = finally_body {
                    collect_return_operands(f, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_return_operands(&c.body, out);
                }
                if let Some(d) = default {
                    collect_return_operands(d, out);
                }
            }
            _ => {}
        }
    }
}
