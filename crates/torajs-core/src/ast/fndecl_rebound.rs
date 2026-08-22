//! A function declaration whose own name gets assigned to.
//!
//! ```text
//! function f() { return "decl"; }
//! f = 123;                          // bun: fine. tr: type error
//! ```
//!
//! A `FunctionDeclaration` creates a plain mutable binding (§14.1.23 —
//! `CreateMutableBinding` + `InitializeBinding`, the same pair a `var`
//! gets), so writing anything at all into it is an ordinary PutValue.
//! tr instead types the name from the declaration — `Function([],
//! String)` — and the checker rejects the write; and even a write of
//! another *function* dies one layer down, because a declaration is a
//! direct-call symbol and never had a slot to write into at all
//! (`ssa-lower: assign to unknown ident`).
//!
//! Both halves are the same missing thing: a slot. So this pass gives
//! the declaration one, out of the rewrite
//! [`super::nested_fns_capture`] already performs for a different
//! reason — a `let` holding a function expression, which
//! [`super::lift_arrow_fns`] then lifts exactly as it lifts an arrow.
//! `any` is what makes the write legal: the Any-let lane already
//! carries a binding that holds a function now and a number later
//! (probe `let f: any = function(){}; f = 123` answers `123` /
//! `number` today), so the whole fix is declaring such bindings `any`
//! up front — the same shape, and the same sentence, as
//! [`crate::let_widen`].
//!
//! ## What routes
//!
//! Per statement list, over its direct non-synthetic `function`
//! declarations: the name is assigned somewhere in that list (nested
//! bodies included — the annexB block-function tests write the binding
//! from inside the declaration's *own* body). Unlike
//! `nested_fns_capture`, the module's own list routes too: a top-level
//! `function f(){}; f = 1` is the plainest form of the thing, and a
//! top-level `let` becomes a global slot, which is exactly what the
//! write needs.
//!
//! Two guards keep the rewrite from taking anything it would answer
//! worse than today:
//!
//! 1. **Hoisting.** The rewrite stays where the declaration was, so the
//!    name stops being usable before it. A declaration whose name is
//!    free in anything ahead of it therefore does not route. This is
//!    the boundary `nested_fns_capture` records, taken here as a
//!    precondition rather than an accepted loss: everything that works
//!    today keeps working.
//! 2. **Shape.** A generator, a generic, or a body naming `arguments`
//!    cannot faithfully become a function expression — those keep the
//!    declaration form and today's error.
//!
//! Recorded boundary: the assigned-name scan is by name, so a `let f`
//! declared in some nested block of the same list, reassigned there,
//! routes an unrelated `function f` too. That costs the typed lane for
//! one binding and nothing else — the Any lane answers the same
//! program.

use super::free_vars::free_vars_of_body;
use super::{Ast, Expr, Stmt};

/// Names the compiler mints (lifted arrows, class methods, generator
/// state machines, forwarders, an earlier nested lift) are never a
/// user's function declaration.
fn is_synthetic(name: &str) -> bool {
    name.starts_with("__")
}

pub fn widen_rebound_fn_decls(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    // Function-expression / arrow bodies live in the expression arena,
    // not the statement tree; the arena holds every such body flat, so
    // a fixed-length low→high scan reaches each exactly once. Same
    // traversal as `nested_fns_capture`, for the same reason.
    let n = ast.exprs.len();
    for i in 0..n {
        if !matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            continue;
        }
        let mut body = match &mut ast.exprs[i] {
            Expr::ArrowFn { body, .. } => std::mem::take(body),
            _ => unreachable!("matched ArrowFn above"),
        };
        walk_list(ast, &mut body);
        match &mut ast.exprs[i] {
            Expr::ArrowFn { body: b, .. } => *b = body,
            _ => unreachable!("matched ArrowFn above"),
        }
    }
    walk_list(ast, &mut top);
    ast.stmts = top;
}

fn walk_list(ast: &mut Ast, stmts: &mut Vec<Stmt>) {
    for s in stmts.iter_mut() {
        walk_children(ast, s);
    }
    route_list(ast, stmts);
}

/// Descend into every statement list `s` owns.
fn walk_children(ast: &mut Ast, s: &mut Stmt) {
    match s {
        Stmt::FnDecl { body, .. } => walk_list(ast, body),
        Stmt::Block(b) | Stmt::Multi(b) => walk_list(ast, b),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk_children(ast, then_branch);
            if let Some(eb) = else_branch.as_deref_mut() {
                walk_children(ast, eb);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::Labeled { body, .. } => walk_children(ast, body),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_list(ast, body);
            walk_list(ast, catch_body);
            if let Some(fb) = finally_body {
                walk_list(ast, fb);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                walk_list(ast, &mut c.body);
            }
            if let Some(d) = default {
                walk_list(ast, d);
            }
        }
        _ => {}
    }
}

/// One statement list: which of its `function` declarations get a slot.
fn route_list(ast: &mut Ast, stmts: &mut [Stmt]) {
    let sites: Vec<usize> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl { name, .. } if !is_synthetic(name) => Some(i),
            _ => None,
        })
        .collect();
    if sites.is_empty() {
        return;
    }
    let assigned =
        crate::ssa_lower_closure_captures::collect_assigned_names_scoped(ast, stmts.iter(), &[]);
    if assigned.is_empty() {
        return;
    }
    for i in sites {
        let name = match &stmts[i] {
            Stmt::FnDecl { name, .. } => name.clone(),
            _ => unreachable!("site is a FnDecl"),
        };
        if !assigned.contains(&name) {
            continue;
        }
        if read_before(ast, &stmts[..i], &name) || !can_rewrite(ast, &stmts[i]) {
            continue;
        }
        rewrite_in_place(ast, &mut stmts[i]);
    }
}

/// Is `name` free in anything that sits before the declaration? The
/// rewrite stays in place, so the name stops being usable before it —
/// a program that reads it earlier keeps the declaration form and
/// today's answer. Deliberately the coarse question: a preceding
/// declaration's body only *holds* the name for later, but counting it
/// costs one un-routed program and never a wrong one.
fn read_before(ast: &Ast, earlier: &[Stmt], name: &str) -> bool {
    free_vars_of_body(ast, &[], earlier)
        .iter()
        .any(|n| n == name)
}

/// A function expression has no type parameters, is never a generator,
/// and does not own `arguments` — a declaration wanting any of those
/// keeps its declaration form and today's answer.
fn can_rewrite(ast: &Ast, s: &Stmt) -> bool {
    let Stmt::FnDecl {
        type_params,
        is_generator,
        params,
        body,
        ..
    } = s
    else {
        unreachable!("site is a FnDecl");
    };
    if *is_generator || !type_params.is_empty() {
        return false;
    }
    let prebound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    !free_vars_of_body(ast, &prebound, body)
        .iter()
        .any(|n| n == "arguments")
}

/// `function f(p): R { … }` →
/// `let f: any = undefined; f = function (p): R { … };` in the slot the
/// declaration occupied.
///
/// **Declaration and initialization are two statements on purpose.** A
/// rebound declaration usually names itself — the annexB block-function
/// tests write the binding from inside the very body being stored — and
/// as one statement that closure captures a binding that does not exist
/// yet (`ssa-lower: closure capture … not in scope`). Split, the name is
/// declared before the expression is built, so the capture resolves; and
/// because the name is also written, the escape-capture pre-pass gives
/// it the box that makes the write visible on both sides. Both halves
/// were already there — this pass only has to ask for them in that
/// order.
///
/// `Multi` rather than `Block`: its children land in the surrounding
/// scope, which is where the declaration's binding lived.
///
/// Registering the value as a function expression (rather than letting
/// it read as an arrow) is what gives the lifted closure the
/// `.prototype` a declaration has; the span carries over so the lifted
/// decl still points at the user's text.
fn rewrite_in_place(ast: &mut Ast, slot: &mut Stmt) {
    let taken = std::mem::replace(slot, Stmt::Block(Vec::new()));
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        ..
    } = taken
    else {
        unreachable!("site is a FnDecl");
    };
    let fn_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(fn_eid, span);
    ast.fn_expr_exprs.insert(fn_eid);
    let undef = ast.add_expr(Expr::Ident("undefined".into()));
    let target = ast.add_expr(Expr::Ident(name.clone()));
    let assign = ast.add_expr(Expr::Assign {
        target,
        value: fn_eid,
    });
    *slot = Stmt::Multi(vec![
        Stmt::LetDecl {
            mutable: true,
            name,
            type_ann: Some("any".to_string()),
            init: undef,
            is_var: false,
        },
        Stmt::Expr(assign),
    ]);
}
