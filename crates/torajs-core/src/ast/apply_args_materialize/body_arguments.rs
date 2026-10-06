//! A body-level `arguments` declaration under parameter expressions.
//!
//! §10.2.11: when a function has parameter expressions, a function
//! or lexical declaration named `arguments` in its body does not
//! suppress the arguments object (steps 18-22 only drop it for such
//! a declaration when there are NO parameter expressions), and the
//! body's declarations live in their own environment (steps 27-28).
//! So `function (x = args = arguments) { function arguments() {} }`
//! reads the arguments object in the parameter list and the local
//! function in the body.
//!
//! Once the defaults move into the body as guards, both readers sit
//! in one statement list and resolve to the same name. Renaming each
//! body declaration, with the references in its own scope, to
//! [`BODY_ARGUMENTS`] before the guards splice in gives each its own
//! binding again: a block-level `let arguments` renames only inside
//! its block, and every read outside a declaring scope — the guards'
//! included — is then the object the arguments pass builds. The
//! rename does not enter nested non-arrow functions or object literal
//! methods, which have their own `arguments`. A body that also has a
//! `var arguments` keeps the whole-function shadow it had: step 28
//! seeds that var from the object, a different shape.

use super::super::desugar_with::walk::{
    expr_children, stmt_children, stmt_children_ref, stmt_exprs,
};
use super::{Ast, Expr, ExprId, Stmt};

/// The body binding's spelling. A nested function declared under it
/// still reports `.name` "arguments" (`annexb_fn_var` maps it back).
pub(crate) const BODY_ARGUMENTS: &str = "__torajs_body_arguments";

/// Rename every function or lexical `arguments` binding the body
/// declares, each with the references in its own scope, so the
/// statements outside every such scope (the guards included) keep
/// reading the arguments object. Leaves the body untouched when it
/// declares none, when it has a `var arguments`, or when an arrow
/// inside it declares its own (that would need the rename inside the
/// arrow's body).
pub(super) fn split_body_arguments(ast: &mut Ast, body: &mut [Stmt]) {
    if !body.iter().any(declares_arguments) || body.iter().any(declares_var_arguments) {
        return;
    }
    let mut refs: Vec<ExprId> = Vec::new();
    let active = body.iter().any(scope_decl);
    if !body.iter().all(|s| collect_refs(ast, s, active, &mut refs)) {
        return;
    }
    for id in refs {
        ast.exprs[id.0 as usize] = Expr::Ident(BODY_ARGUMENTS.to_string());
    }
    for s in body.iter_mut() {
        rename_decls(s);
    }
}

fn declares_arguments(s: &Stmt) -> bool {
    scope_decl(s) || stmt_children_ref(s).into_iter().any(declares_arguments)
}

/// A declaration this statement adds to the scope it sits in. A
/// `Multi` is a synthetic grouping, not a scope. A block-level
/// `function arguments` stays in its block even in sloppy code:
/// Annex B.3.2.1 never hoists the name `arguments`.
fn scope_decl(s: &Stmt) -> bool {
    match s {
        Stmt::FnDecl { name, .. } => name == "arguments",
        Stmt::LetDecl { name, is_var, .. } => name == "arguments" && !is_var,
        Stmt::Multi(v) => v.iter().any(scope_decl),
        _ => false,
    }
}

fn declares_var_arguments(s: &Stmt) -> bool {
    match s {
        Stmt::LetDecl { name, is_var, .. } => name == "arguments" && *is_var,
        _ => stmt_children_ref(s).into_iter().any(declares_var_arguments),
    }
}

/// The `arguments` reads that resolve to a body binding: those inside
/// a statement list (or a loop / switch / try head) that declares one,
/// arrow bodies included and nested functions excluded. `false` when
/// an arrow body declares its own `arguments`.
fn collect_refs(ast: &Ast, s: &Stmt, active: bool, out: &mut Vec<ExprId>) -> bool {
    if matches!(s, Stmt::FnDecl { .. }) {
        return true;
    }
    let children = stmt_children_ref(s);
    let inner = active || children.iter().any(|c| scope_decl(c));
    stmt_exprs(s)
        .into_iter()
        .all(|e| collect_expr_refs(ast, e, inner, out))
        && children
            .into_iter()
            .all(|c| collect_refs(ast, c, inner, out))
}

fn collect_expr_refs(ast: &Ast, e: ExprId, active: bool, out: &mut Vec<ExprId>) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) if n == "arguments" => {
            if active {
                out.push(e);
            }
            true
        }
        Expr::ArrowFn { body, .. } => {
            // a function expression, generator expression or object
            // literal method (getters and setters included) has its
            // own `arguments`
            if ast.fn_expr_exprs.contains(&e)
                || ast.gen_fn_exprs.contains_key(&e)
                || ast.objlit_method_exprs.contains(&e)
            {
                return true;
            }
            !body.iter().any(declares_arguments)
                && body.iter().all(|s| collect_refs(ast, s, active, out))
        }
        _ => expr_children(ast, e)
            .into_iter()
            .all(|c| collect_expr_refs(ast, c, active, out)),
    }
}

fn rename_decls(s: &mut Stmt) {
    match s {
        Stmt::FnDecl { name, .. } if name == "arguments" => *name = BODY_ARGUMENTS.to_string(),
        Stmt::LetDecl { name, is_var, .. } if name == "arguments" && !*is_var => {
            *name = BODY_ARGUMENTS.to_string()
        }
        Stmt::FnDecl { .. } => {}
        _ => {
            for c in stmt_children(s) {
                rename_decls(c);
            }
        }
    }
}
