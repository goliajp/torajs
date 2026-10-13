//! The rewrite itself: a routed declaration becomes a binding holding
//! its function value. Moved out of the parent when it neared the
//! file-size limit; the parent keeps which declarations route.

use crate::ast::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// Move to the head of a function body every declaration whose binding
/// a written `var` of the body shares
/// (`nested_fns_top::var_shared_fn_names`), keeping their order, and
/// answer their names. §10.2.11 step 36 initializes the binding with
/// the function before any statement runs, so the routed form has to
/// sit above every `var` initializer and assignment, not where the
/// declaration's text was. A declaration a sloppy eval spliced in
/// (blanked span, `Ast::sloppy_eval_fn_names`) stays put: it is
/// initialized where the eval ran (§19.2.1.3 step 17).
pub(super) fn raise_var_shared(ast: &Ast, stmts: &mut [Stmt]) -> HashSet<String> {
    let shared = crate::ast::nested_fns_top::var_shared_fn_names(ast, stmts);
    // below the directive prologue, which has to stay the body's head
    let start = crate::ast::sloppy_this_prologue::directive_prologue_len(stmts, &ast.exprs);
    let mut head = start;
    for i in start..stmts.len() {
        if matches!(&stmts[i], Stmt::FnDecl { name, span, .. } if shared.contains(name)
            && !(span.start == 0 && span.end == 0 && ast.sloppy_eval_fn_names.contains(name)))
        {
            stmts[head..=i].rotate_right(1);
            head += 1;
        }
    }
    shared
}

/// `function f(p): R { … }` → `let f = function (p): R { … }`, in the
/// slot the declaration occupied. Registering the expression as a
/// function expression (rather than leaving it to read as an arrow) is
/// what gives the lifted closure its `.prototype`, which a declaration
/// has; the span carries over so the lifted decl still points at the
/// user's text.
///
/// `shared`: a written `var` shares the binding, so the rewrite is a
/// `var` too — one slot for both, as the var hoist makes of any
/// re-declared name.
pub(super) fn rewrite_in_place(ast: &mut Ast, slot: &mut Stmt, shared: bool) {
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
        mutable: shared,
        name,
        type_ann: None,
        init: eid,
        is_var: shared,
    };
}
