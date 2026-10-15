//! `f?.(x)` with `f` a name bound to a function expression reading
//! `this`: the guard and the call spelled apart,
//!
//! ```text
//! f?.(x)  →  f == null ? undefined : f(x)
//! ```
//!
//! §13.3.9: the optional call answers `undefined` when `f` is undefined
//! or null, without evaluating the arguments; otherwise it is the call
//! `f(x)`, whose receiver is `undefined` (§13.3.6.2, a binding supplies
//! none). Reading the name twice is not observable. The receiver
//! passes key on that direct call; the optional spelling was refused.

use crate::ast::{Ast, BinOp, Expr, ExprId};

pub(super) fn guard(ast: &mut Ast, i: usize) {
    let Expr::OptCall { callee, args } = ast.exprs[i].clone() else {
        return;
    };
    let Expr::Ident(name) = ast.get_expr(callee).clone() else {
        return;
    };
    let span = ast.expr_spans[i];
    let add = |ast: &mut Ast, e: Expr| {
        let id = ast.add_expr(e);
        ast.set_expr_span(id, span);
        id
    };
    let read = add(ast, Expr::Ident(name));
    let null = add(ast, Expr::Null);
    let cond = add(
        ast,
        Expr::BinOp {
            op: BinOp::LooseEq,
            left: read,
            right: null,
        },
    );
    let undef = add(ast, Expr::Ident("undefined".into()));
    let call = add(ast, Expr::Call { callee, args });
    if let Some(t) = ast.call_type_args.remove(&ExprId(i as u32)) {
        ast.call_type_args.insert(call, t);
    }
    ast.exprs[i] = Expr::Ternary {
        cond,
        then_branch: undef,
        else_branch: call,
    };
}
