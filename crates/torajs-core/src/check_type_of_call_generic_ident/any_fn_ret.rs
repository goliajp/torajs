//! The type of a generic call that hands an `any` argument to a
//! fn-typed parameter, the generic twin of
//! `check_type_of_call::any_widen_ret`.
//!
//! The instantiation takes that parameter as `any`
//! (`check_monomorph::any_fn_param`) and calls whatever the box holds,
//! which answers what that function returns, not the type the
//! parameter declares (`first<number>(objs.wide, 7)` is the string
//! `"w7"`). So the call types `any` and the instantiation answers
//! `any`, unless the static return sniff, re-run with the widened
//! parameters `any` and the others at their type arguments, still
//! names the declared type.

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, Type};
use crate::check_type_ann::resolve_type_ann_full;

/// The type of generic call `eid` to `name`, whose substituted return
/// is `ret`; `widened` are the parameters that take `any`.
pub(super) fn call_ret_face(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    name: &str,
    widened: &[usize],
    subst: &HashMap<String, Type>,
    ret: Type,
) -> Type {
    if widened.is_empty()
        || checker.widen_ret_declared
        || matches!(ret, Type::Void | Type::Never | Type::Undefined | Type::Any)
    {
        return ret;
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name: n,
            params,
            body,
            ..
        } if n == name => Some((params, body)),
        _ => None,
    }) else {
        return ret;
    };
    let pairs: Vec<(String, String)> = subst
        .iter()
        .map(|(tv, t)| (tv.clone(), crate::check_type_to_ann::type_to_ann(t)))
        .collect();
    let mut params = params.clone();
    for (i, p) in params.iter_mut().enumerate() {
        p.type_ann = if widened.contains(&i) {
            Some("any".to_string())
        } else {
            p.type_ann
                .as_deref()
                .map(|a| crate::ssa_lower_generics_monomorph::substitute_in_ann(a, &pairs))
        };
    }
    let kept = crate::ast::infer_return_ann(&ast.exprs, body, &params, &HashMap::new())
        .and_then(|ann| {
            resolve_type_ann_full(&ann, &checker.aliases, &[], &checker.generic_alias_decls)
        })
        .is_some_and(|t| t == ret);
    if kept {
        return ret;
    }
    checker.widen_ret_any.insert(eid);
    Type::Any
}
