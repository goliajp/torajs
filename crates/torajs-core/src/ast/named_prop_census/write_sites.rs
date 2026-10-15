//! Which expressions of the program may write a member
//! (`super::write_hit` says what each one reaches).

use super::write_hit::descriptor_getter;
use crate::ast::{Ast, Expr, ExprId};

/// The expressions that may write a member: a store or a step through
/// a member or a key, a call spelled `Reflect.set` / `Object.assign`
/// or one that hands out a setter, and, with `literals`, an object literal (an expression of any other
/// kind reaches no name, whatever its type). A generic body's writes
/// are left out: it never lowers, the checker types nothing in it, and
/// each specialization the mono pass clones from it is a body of its
/// own, typed and counted in the census over the mono program.
pub(super) fn candidates(ast: &Ast, literals: bool) -> Vec<ExprId> {
    let member = |t: &ExprId| matches!(ast.get_expr(*t), Expr::Member { .. } | Expr::Index { .. });
    let generic = generic_bodies(ast);
    (ast.exprs.iter().enumerate())
        .filter(|(i, e)| {
            !generic.get(*i).is_some_and(|g| *g)
                && match e {
                    Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => member(target),
                    Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => bulk(ast, *callee),
                    Expr::ObjectLit { .. } => literals,
                    _ => false,
                }
        })
        .map(|(i, _)| ExprId(i as u32))
        .collect()
}

/// One flag per arena slot: the expression sits in the body (or a
/// parameter default) of a function declared with type parameters.
/// Empty when the program declares none.
fn generic_bodies(ast: &Ast) -> Vec<bool> {
    use crate::ast::Stmt;
    use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
    fn mark(ast: &Ast, e: ExprId, out: &mut [bool]) {
        if !std::mem::replace(&mut out[e.0 as usize], true) {
            visit_expr_children(ast, e, &mut |c| mark(ast, c, out));
        }
    }
    fn walk(ast: &Ast, s: &Stmt, out: &mut Vec<bool>) {
        match s {
            Stmt::FnDecl {
                type_params,
                params,
                body,
                ..
            } if !type_params.is_empty() => {
                if out.is_empty() {
                    *out = vec![false; ast.exprs.len()];
                }
                let mut add = |e: ExprId| mark(ast, e, out);
                for p in params {
                    if let Some(d) = p.default {
                        add(d);
                    }
                }
                for st in body {
                    visit_stmt_exprs(st, &mut add);
                }
            }
            Stmt::Multi(v) | Stmt::Block(v) => {
                for st in v {
                    walk(ast, st, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in &ast.stmts {
        walk(ast, s, &mut out);
    }
    out
}

/// A callee spelled `Reflect.set` or `Object.assign`, or one that hands
/// out a setter ([`super::write_hit::call`]).
fn bulk(ast: &Ast, callee: ExprId) -> bool {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return false;
    };
    name == "__lookupSetter__"
        || matches!(ast.get_expr(*obj), Expr::Ident(b)
        if (b == "Reflect" && name == "set") || (b == "Object" && name == "assign")
            || descriptor_getter(b, name))
}
