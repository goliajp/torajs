//! `hs.map((f) => f(2))` over an array of the element census
//! (`ast::face_slots::FaceSlots::elems`): the walk hands each element to
//! the callback's element parameter, so a function value of another face
//! stored in the array reaches that parameter too. The parameter joins
//! the boxed-entry census (`face_slots::record_param`), whose calls ride
//! the adapter that unboxes per the stored value's own face, as a
//! direct call handing it such a value does.

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, FnSlot, Type};

/// Record the element parameters of the callback of `recv.m(cb, …)`
/// when `recv` is an array of the census, once a route has typed the
/// call: a callback body checked before the record reads the census
/// again in the next run (`face_slots::param_read`).
pub(crate) fn record(checker: &mut Checker, ast: &Ast, callee: ExprId, args: &[ExprId]) {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return;
    };
    // the positions of the callback's parameters an element reaches
    let elem_pos: &[usize] = match name.as_str() {
        "forEach" | "map" | "filter" | "some" | "every" | "find" | "findIndex" | "findLast"
        | "findLastIndex" | "flatMap" => &[0],
        // a function accumulator holds the first element when unseeded,
        // and whatever element the callback returns
        "reduce" | "reduceRight"
            if args.get(1).is_none_or(|&seed| {
                (checker.expr_types.get(&seed)).is_some_and(crate::check::face_slots::is_fn_elem)
            }) =>
        {
            &[0, 1]
        }
        "reduce" | "reduceRight" => &[1],
        "sort" | "toSorted" => &[0, 1],
        _ => return,
    };
    let Some(&cb) = args.first() else {
        return;
    };
    let fn_name = match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name.clone(),
        // a nested declaration that captures is bound to its lifted
        // closure (`const each = __closure_3`)
        Expr::Ident(g) => match checker.lookup(g).and_then(|info| info.decl) {
            Some(FnSlot::Let(init)) => match ast.get_expr(init) {
                Expr::Closure { fn_name, .. } => fn_name.clone(),
                _ => g.clone(),
            },
            _ => g.clone(),
        },
        _ => return,
    };
    let fn_name = fn_name.as_str();
    let Some(recv @ Type::Array(_)) = checker.expr_types.get(obj).cloned() else {
        return;
    };
    if crate::check::face_slots::elem_read(checker, &recv).is_none() {
        return;
    }
    for &i in elem_pos {
        if let Some(p) = user_param_name(ast, fn_name, i) {
            crate::check::face_slots::record_param(checker, fn_name, &p);
        }
    }
}

/// The name of user parameter `i` of the declaration `name`, past a
/// lifted body's `__env` head and a hidden argv slot.
fn user_param_name(ast: &Ast, name: &str, i: usize) -> Option<String> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n, params, ..
        } if n == name => {
            let user = &params[usize::from(params.first().is_some_and(|p| p.name == "__env"))..];
            let hidden = crate::check_stmt_fn_decl::argv_slot_hidden(name, user);
            user.iter()
                .filter(|p| !(hidden && p.name == "__torajs_argv"))
                .nth(i)
                .map(|p| p.name.clone())
        }
        _ => None,
    })
}
