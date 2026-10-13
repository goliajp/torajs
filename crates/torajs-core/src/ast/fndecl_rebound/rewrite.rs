//! The rewrite itself: a declaration becomes a slot plus the write of
//! its function value. Moved out of the parent when it neared the
//! file-size limit; the parent keeps which declarations route.

use crate::ast::{Ast, Expr, Stmt};

/// `function f(p): R { … }` →
/// `let f: any = undefined; f = function (p): R { … };` in the slot the
/// declaration occupied.
///
/// **Declaration and initialization are two statements on purpose.** A
/// rebound declaration usually names itself — the annexB block-function
/// tests write the binding from inside the very body being stored — and
/// as one statement that closure captures a binding that does not exist
/// yet (`ssa-lower: closure capture … not in scope`). Split, the name is
/// declared before the expression is built, so the capture resolves; and
/// because the name is also written, the escape-capture pre-pass gives
/// it the box that makes the write visible on both sides. Both halves
/// were already there — this pass only has to ask for them in that
/// order.
///
/// `Multi` rather than `Block`: its children land in the surrounding
/// scope, which is where the declaration's binding lived.
///
/// Registering the value as a function expression (rather than letting
/// it read as an arrow) is what gives the lifted closure the
/// `.prototype` a declaration has; the span carries over so the lifted
/// decl still points at the user's text.
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
    let fn_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(fn_eid, span);
    ast.fn_expr_exprs.insert(fn_eid);
    let undef = ast.add_expr(Expr::Ident("undefined".into()));
    let target = ast.add_expr(Expr::Ident(name.clone()));
    let assign = ast.add_expr(Expr::Assign {
        target,
        value: fn_eid,
    });
    *slot = Stmt::Multi(vec![
        Stmt::LetDecl {
            mutable: true,
            name,
            type_ann: Some("any".to_string()),
            init: undef,
            is_var: false,
        },
        Stmt::Expr(assign),
    ]);
}
