//! Reading a generator's completion value back out of an `any` slot.

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};

/// `this.<slot>` — an `any` field that holds a completion value
/// (`return(v)`'s injected value, a `return v` held across a
/// `finally`) — read back at the step's value type `ty`, as the two
/// returns `out` takes: `undefined` first, then the value itself.
///
/// `undefined` is the value most of these carry: a for-of or
/// destructuring close calls `return()` with none (§7.4.9), and a bare
/// `return;` completes with it. A plain `as T` refused it for a
/// pointer-shaped `T` ("value is not an array"), and so does one
/// behind a conditional expression, whose join is `any`; it is
/// returned as its own statement, spelled the way the slot's own
/// completion value is (`completion_undefined`). The any lane's single
/// `as any` holds it as it is.
pub(crate) fn typed_returns(
    ast: &mut Ast,
    slot: &str,
    ty: &str,
    mut out: impl FnMut(&mut Ast, ExprId) -> Stmt,
) -> Vec<Stmt> {
    let read = |ast: &mut Ast| {
        let obj = ast.add_expr(Expr::This);
        ast.add_expr(Expr::Member {
            obj,
            name: slot.to_string(),
        })
    };
    if ty == "any" {
        let expr = read(ast);
        let val = ast.add_expr(Expr::As {
            expr,
            ty_ann: ty.to_string(),
        });
        return vec![out(ast, val)];
    }
    let probe = read(ast);
    let undef = ast.add_expr(Expr::Ident("undefined".into()));
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: probe,
        right: undef,
    });
    let undef_val = ast.add_expr(crate::ast::desugar_generators_methods::completion_undefined(ty));
    let undef_ret = out(ast, undef_val);
    let held = read(ast);
    let typed_val = ast.add_expr(Expr::As {
        expr: held,
        ty_ann: ty.to_string(),
    });
    let typed_ret = out(ast, typed_val);
    vec![
        Stmt::If {
            cond,
            then_branch: Box::new(undef_ret),
            else_branch: None,
        },
        typed_ret,
    ]
}

/// `val` as it is stored into an `any` slot: a typed slot's
/// `undefined` (the `__undef_slot__<T>` marker) would land there as
/// T's raw sentinel cell, not as undefined, so it is spelled plainly.
pub(crate) fn any_spelling(ast: &mut Ast, val: ExprId) -> ExprId {
    match ast.get_expr(val) {
        Expr::Ident(n) if n.starts_with(crate::ast::UNDEF_SLOT_MARKER) => {
            ast.add_expr(Expr::Ident("undefined".into()))
        }
        _ => val,
    }
}
