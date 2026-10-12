//! A function declaration of an async or generator body turned into a
//! value binding (`select`'s `Route::Value`).

use super::super::{Ast, Expr, Stmt};

/// `function f(p): R { … }` → `let f = function (p): R { … }`; an async
/// declaration's value is marked async.
pub(super) fn to_value_binding(ast: &mut Ast, s: Stmt) -> Stmt {
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        is_generator,
        ..
    } = s
    else {
        unreachable!("caller checked");
    };
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    if is_generator {
        // An async generator expression, the parser's own marks.
        ast.gen_fn_exprs.insert(
            eid,
            super::super::GenFnExprInfo {
                kind: super::super::GenFnExprKind::AsyncGenerator,
                destr_prefix: 0,
            },
        );
    } else {
        ast.fn_expr_exprs.insert(eid);
        if ast.async_fns.contains(&name) {
            ast.async_fn_value_exprs.insert(eid);
        }
    }
    Stmt::LetDecl {
        mutable: false,
        name,
        type_ann: None,
        init: eid,
        is_var: false,
    }
}
