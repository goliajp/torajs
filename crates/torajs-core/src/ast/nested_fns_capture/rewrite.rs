//! The rewrite itself: a routed declaration becomes a binding holding
//! its function value. Moved out of the parent when it neared the
//! file-size limit; the parent keeps which declarations route.

use crate::ast::{Ast, Expr, Stmt};

/// `function f(p): R { … }` → `let f = function (p): R { … }`, in the
/// slot the declaration occupied. Registering the expression as a
/// function expression (rather than leaving it to read as an arrow) is
/// what gives the lifted closure its `.prototype`, which a declaration
/// has; the span carries over so the lifted decl still points at the
/// user's text.
pub(super) fn rewrite_in_place(ast: &mut Ast, slot: &mut Stmt) {
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
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    ast.fn_expr_exprs.insert(eid);
    // a value has no static callee to pad its defaults at
    crate::ast::apply_args_materialize::materialize_declared_fn_value(ast, eid, &name);
    *slot = Stmt::LetDecl {
        mutable: false,
        name,
        type_ann: None,
        init: eid,
        is_var: false,
    };
}
