//! §14.11 + §14.3.2 — a `var` declaration in a `with` body is two
//! things living in two different scopes.
//!
//! The DECLARATION belongs to the enclosing function's variable
//! environment, which sits BEHIND the object record: nothing resolves
//! it through the object, and it exists for the whole function.
//! The INITIALISER is an assignment evaluated where it is written, IN
//! FRONT of that record — so when the object carries the name it
//! writes `o.v`, and the hoisted binding is left undefined.
//!
//! ```text
//! with ({ v: 1 }) { var v = 2 }   // bun: o.v === 2, v === undefined
//! ```
//!
//! Splitting the statement into `var v; v = 2` hands each half to the
//! pass that already gets it right: `desugar_var_hoist` lifts the
//! declaration to the function head, and the assignment becomes an
//! ordinary write site this desugar guards. Nothing here has to know
//! how either of those works.

use super::walk::stmt_children;
use crate::ast::{Ast, Expr, Stmt};

/// Split every `var <name> = <init>` in one `with` body. Run before
/// the site collection, which then sees the assignment half.
pub(crate) fn split_var_inits(ast: &mut Ast, stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        split_in_stmt(ast, s);
    }
}

fn split_in_stmt(ast: &mut Ast, s: &mut Stmt) {
    match s {
        // A nested function's `var` is an ordinary local of THAT
        // function, bound in front of the object. Nothing to split,
        // and splitting would move its declaration to the wrong head.
        Stmt::FnDecl { .. } => return,
        Stmt::For { body, .. } => {
            split_in_stmt(ast, body);
            split_for_init(ast, s);
            return;
        }
        _ => {
            for child in stmt_children(s) {
                split_in_stmt(ast, child);
            }
        }
    }
    let Stmt::LetDecl {
        name,
        init,
        is_var: true,
        ..
    } = s
    else {
        return;
    };
    // `var v;` declares and writes nothing — there is no assignment to
    // put in front of the object record.
    if matches!(ast.get_expr(*init), Expr::Uninit) {
        return;
    }
    let name = name.clone();
    let value = *init;
    let decl = uninit_decl(ast, &name);
    let assign = assign_to_name(ast, &name, value);
    *s = Stmt::Multi(vec![decl, Stmt::Expr(assign)]);
}

/// `for (var i = 0; …)` — the same split, except the two halves cannot
/// stay where they are: the init slot holds ONE statement and the loop
/// lowering has never seen a `Multi` there. So the declaration is
/// lifted out in front of the loop and the slot keeps only the
/// assignment, which is a shape the loop already accepts (`for (i = 0;
/// …)`). The loop's own structure is untouched.
///
/// Several declarators (`for (var i = 0, j = 1; …)`) arrive as a
/// `Multi` in the slot and are left alone — the collect walk still
/// refuses them by name rather than this guessing at a shape it has
/// not seen.
fn split_for_init(ast: &mut Ast, s: &mut Stmt) {
    let Stmt::For {
        init: Some(init_stmt),
        ..
    } = s
    else {
        return;
    };
    let Stmt::LetDecl {
        name,
        init,
        is_var: true,
        ..
    } = init_stmt.as_ref()
    else {
        return;
    };
    if matches!(ast.get_expr(*init), Expr::Uninit) {
        return;
    }
    let name = name.clone();
    let value = *init;
    let decl = uninit_decl(ast, &name);
    let assign = assign_to_name(ast, &name, value);
    let Stmt::For { init: slot, .. } = s else {
        return;
    };
    *slot = Some(Box::new(Stmt::Expr(assign)));
    let loop_stmt = std::mem::replace(s, Stmt::Multi(Vec::new()));
    *s = Stmt::Multi(vec![decl, loop_stmt]);
}

/// The declaration half: `var <name>;`.
///
/// The annotation is dropped on purpose — `desugar_var_hoist` treats
/// an explicitly annotated `var` as block-scoped and leaves it where
/// it stands, which is the one placement §14.11 cannot have. The
/// declaration has to reach the function head, behind the object.
fn uninit_decl(ast: &mut Ast, name: &str) -> Stmt {
    let init = ast.add_expr(Expr::Uninit);
    Stmt::LetDecl {
        mutable: true,
        name: name.to_string(),
        type_ann: None,
        init,
        is_var: true,
    }
}

/// The write half: `<name> = <value>`, an ordinary assignment the
/// `with` rewrite guards like any other.
fn assign_to_name(ast: &mut Ast, name: &str, value: crate::ast::ExprId) -> crate::ast::ExprId {
    let target = ast.add_expr(Expr::Ident(name.to_string()));
    ast.add_expr(Expr::Assign { target, value })
}
