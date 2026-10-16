//! A write to a `const` from inside a nested function is a runtime
//! TypeError.
//!
//! §9.1.1.1.5 SetMutableBinding on an immutable binding throws a
//! TypeError — in strict AND sloppy code, `const` bindings being
//! strict-immutable — after the right-hand side has been evaluated
//! (§13.15.2 PutValue follows GetValue of the rref). The checker
//! answers the same-scope spelling itself (`const c = 3; c = 4` is
//! marked and the assign lane throws). A write from a NESTED function
//! got past it: for a top-level `const` it died in the lowering
//! ("assign to unknown ident"), since an inlined-literal `const` has
//! no slot for a store to land in; for a `const` declared in a
//! function body or a block, the closure's captured copy took the
//! store and the program ran on with a wrong value. test262
//! `global-code/decl-lex` and `staging/sm/regress/regress-383902`
//! assert the throw.
//!
//! The rewrite reads the binding, then mints the readonly-global
//! lane's throw — `(rhs, c, throw)` for an assignment, `(+c, throw)`
//! for `c++` — through the scope-aware collector the generator
//! self-name pass uses: a nested function, arrow, block, `try` /
//! `catch` / `finally` list, switch, for head, for-of head, catch
//! parameter or named function expression that rebinds the name owns
//! the writes inside it. Every statement list that declares a `const`
//! is visited — the top level, a function or arrow body, a block, a
//! `try` / `catch` / `finally` list, a switch's clauses, a class
//! member's body, a function inside a parameter default or a field
//! initializer — and so is a `for` whose head declares one (the
//! parser already rewrites a one-name head, and a `for (const x of
//! …)` variable in `throw_on_const_loop_var_writes`). A program that
//! declares no `const` is left as it was.
//!
//! Runs before the sloppy implicit-global synthesis and before
//! anything lifts a function out of its scope.

use super::desugar_with::walk::{arrow_nodes, stmt_children_ref};
use super::hoist_gen_fn_exprs_self_write::{
    ctor_field_inits, rewrite_writes_inside_functions, rewrite_writes_inside_functions_of_for,
};
use super::{Ast, Expr, ExprId, Param, StaticInit, Stmt, SwitchCase};

pub fn rewrite_const_writes_in_functions(ast: &mut Ast) {
    let stmts = std::mem::take(&mut ast.stmts);
    visit_list(ast, &stmts);
    ast.stmts = stmts;
}

/// The `const` names `list` declares at its own level (a `Multi`
/// group shares the list's scope).
fn declared_consts(list: &[Stmt]) -> Vec<String> {
    super::stmts_flat(list)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::LetDecl {
                mutable: false,
                is_var: false,
                name,
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn visit_list(ast: &mut Ast, list: &[Stmt]) {
    for name in declared_consts(list) {
        rewrite_writes_inside_functions(ast, list, &name);
    }
    for s in list {
        visit_stmt(ast, s);
    }
}

fn visit_stmt(ast: &mut Ast, s: &Stmt) {
    for eid in arrow_nodes(ast, s) {
        visit_arrow(ast, eid);
    }
    match s {
        Stmt::FnDecl { params, body, .. } => {
            visit_defaults(ast, params);
            visit_list(ast, body);
        }
        Stmt::Block(body) => visit_list(ast, body),
        // The group's declarations belong to the enclosing list, which
        // has already rewritten for them.
        Stmt::Multi(list) => {
            for c in list {
                visit_stmt(ast, c);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            visit_list(ast, body);
            visit_list(ast, catch_body);
            if let Some(f) = finally_body {
                visit_list(ast, f);
            }
        }
        Stmt::Switch { cases, default, .. } => visit_switch(ast, cases, default.as_deref()),
        Stmt::ClassDecl { .. } => visit_class(ast, s),
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => visit_stmt(ast, inner),
        Stmt::For {
            init: Some(init), ..
        } => {
            for name in declared_consts(std::slice::from_ref(init.as_ref())) {
                rewrite_writes_inside_functions_of_for(ast, s, &name);
            }
            for c in stmt_children_ref(s) {
                visit_stmt(ast, c);
            }
        }
        _ => {
            for c in stmt_children_ref(s) {
                visit_stmt(ast, c);
            }
        }
    }
}

/// §14.12.2 — every clause of a switch shares one scope.
fn visit_switch(ast: &mut Ast, cases: &[SwitchCase], default: Option<&[Stmt]>) {
    let clauses: Vec<&[Stmt]> = cases
        .iter()
        .map(|c| c.body.as_slice())
        .chain(default)
        .collect();
    if clauses.iter().any(|b| !declared_consts(b).is_empty()) {
        let joined: Vec<Stmt> = clauses.iter().flat_map(|b| b.iter().cloned()).collect();
        for name in declared_consts(&joined) {
            rewrite_writes_inside_functions(ast, &joined, &name);
        }
    }
    for c in clauses.into_iter().flatten() {
        visit_stmt(ast, c);
    }
}

/// The instance field initializers spliced into the constructor are
/// not part of its scope (§15.7.10): a `const` the constructor
/// declares does not reach them, so its body is visited without them.
fn visit_class(ast: &mut Ast, s: &Stmt) {
    let Stmt::ClassDecl {
        ctor,
        methods,
        static_methods,
        static_init,
        ..
    } = s
    else {
        return;
    };
    if let Some(c) = ctor {
        visit_defaults(ast, &c.params);
        let fields = ctor_field_inits(ast, &c.body);
        if fields.is_empty() {
            visit_list(ast, &c.body);
        } else {
            let own: Vec<Stmt> = (c.body.iter().enumerate())
                .filter(|(i, _)| !fields.contains(i))
                .map(|(_, s)| s.clone())
                .collect();
            visit_list(ast, &own);
            for f in &c.body[fields] {
                visit_stmt(ast, f);
            }
        }
    }
    for m in methods.iter().chain(static_methods) {
        visit_defaults(ast, &m.params);
        visit_list(ast, &m.body);
    }
    for i in static_init {
        match i {
            StaticInit::Block(b) => visit_list(ast, b),
            StaticInit::Field(f) => visit_stmt(ast, &Stmt::Expr(f.init)),
        }
    }
}

/// The functions written in parameter defaults.
fn visit_defaults(ast: &mut Ast, params: &[Param]) {
    for d in params.iter().filter_map(|p| p.default) {
        visit_stmt(ast, &Stmt::Expr(d));
    }
}

/// An arrow's body lives in the arena; it is lent out for the visit
/// (the rewrite only touches arena expressions, never this node).
fn visit_arrow(ast: &mut Ast, eid: ExprId) {
    let (defaults, body): (Vec<ExprId>, _) = match &mut ast.exprs[eid.0 as usize] {
        Expr::ArrowFn { params, body, .. } => (
            params.iter().filter_map(|p| p.default).collect(),
            std::mem::take(body),
        ),
        _ => unreachable!("arrow_nodes answers arrow nodes"),
    };
    for d in defaults {
        visit_stmt(ast, &Stmt::Expr(d));
    }
    visit_list(ast, &body);
    if let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[eid.0 as usize] {
        *slot = body;
    }
}
