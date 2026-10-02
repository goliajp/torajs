//! Does a nested function body name a binding of the enclosing frame?

use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
use crate::ast::stmt_nested_lists::for_each_nested_list;
use crate::ast::{Ast, Expr, ExprId, Stmt};

pub(super) fn list(ast: &Ast, body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| stmt(ast, s, name))
}

fn stmt(ast: &Ast, s: &Stmt, name: &str) -> bool {
    if let Stmt::FnDecl { body, .. } = s {
        return list(ast, body, name);
    }
    let mut hit = false;
    visit_stmt_exprs(s, &mut |e| hit |= expr(ast, e, name));
    hit || nested_decls(ast, s, name)
}

/// Function declarations anywhere inside `s`, which the expression walk
/// does not enter.
fn nested_decls(ast: &Ast, s: &Stmt, name: &str) -> bool {
    let mut hit = false;
    for_each_nested_list(s, &mut |l| {
        for st in l {
            hit |= match st {
                Stmt::FnDecl { body, .. } => list(ast, body, name),
                _ => nested_decls(ast, st, name),
            };
        }
    });
    hit
}

fn expr(ast: &Ast, e: ExprId, name: &str) -> bool {
    match ast.get_expr(e) {
        Expr::Ident(n) => n == name,
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == name),
        Expr::ArrowFn { body, .. } => list(ast, body, name),
        _ => {
            let mut hit = false;
            visit_expr_children(ast, e, &mut |c| hit |= expr(ast, c, name));
            hit
        }
    }
}
