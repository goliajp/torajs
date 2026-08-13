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
        // `for (var i = 0; …)` — the init slot holds one statement and
        // the loop's shape depends on it, so this one stays refused by
        // the collect walk rather than split into something the loop
        // lowering has never seen.
        Stmt::For { body, .. } => split_in_stmt(ast, body),
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
    let decl_init = ast.add_expr(Expr::Uninit);
    let target = ast.add_expr(Expr::Ident(name.clone()));
    let assign = ast.add_expr(Expr::Assign { target, value });
    *s = Stmt::Multi(vec![
        Stmt::LetDecl {
            mutable: true,
            name,
            // The annotation is dropped on purpose: `desugar_var_hoist`
            // treats an explicitly annotated `var` as block-scoped and
            // leaves it where it stands, which is the one placement
            // §14.11 cannot have — the declaration has to reach the
            // function head, behind the object.
            type_ann: None,
            init: decl_init,
            is_var: true,
        },
        Stmt::Expr(assign),
    ]);
}
