//! A fn-typed binding or parameter handed on to another slot: the slot
//! holds what the binding holds, whatever face the two annotations
//! share.

use super::super::any_widen::clonable_callee;
use super::{
    class_method_decls, local_closure_decl, objlit_method_decl, receiver_param, user_param,
};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::face_slots::record_param;
use crate::check::{Checker, FnSlot, Type};
use crate::check_stmt_let_decl::fn_face_slot::census_value;

/// The face of the value an immutable fn-typed binding holds when its
/// declaration took that face (`Checker::let_face_anns`): the binding
/// is typed by its annotation, so a call handing it on would otherwise
/// see the annotation's face and shape the callee's calls by it.
pub(crate) fn held_face(checker: &Checker, ast: &Ast, arg: ExprId) -> Option<Type> {
    let Expr::Ident(n) = ast.get_expr(arg) else {
        return None;
    };
    let Some(FnSlot::Let(init)) = checker.lookup(n)?.decl else {
        return None;
    };
    if !checker.let_face_anns.contains_key(&init) {
        return None;
    }
    checker.expr_types.get(&init).cloned()
}

/// When the call of `callee` hands its fn-typed parameter `i` (typed
/// `param_ty`) the value of a census slot (`run2(g)` with `g` boxed),
/// which may be a function of another face than both annotations spell,
/// the parameter of each body the call can run joins the census too.
pub(crate) fn box_census_arg(
    checker: &mut Checker,
    ast: &Ast,
    (callee, i): (ExprId, usize),
    (param_ty, arg): (&Type, ExprId),
) {
    if !matches!(param_ty, Type::Function(..)) || !census_value(checker, ast, arg) {
        return;
    }
    let bodies: Vec<(String, usize)> = match ast.get_expr(callee) {
        Expr::Ident(n) => (clonable_callee(checker, ast, n, i))
            .or_else(|| local_closure_decl(checker, ast, n))
            .map(|b| (b, i))
            .into_iter()
            .collect(),
        Expr::Member { obj, name } => {
            let mut bodies = class_method_decls(checker, ast, *obj, name);
            bodies.extend(objlit_method_decl(checker, ast, *obj, name));
            // the receiver binds the body's `__this`, not an argument
            (bodies.into_iter())
                .map(|b| {
                    let at = i + usize::from(receiver_param(ast, &b));
                    (b, at)
                })
                .collect()
        }
        _ => Vec::new(),
    };
    for (body, at) in bodies {
        if let Some((p, _)) = user_param(ast, &body, at) {
            record_param(checker, &body, &p);
        }
    }
}
