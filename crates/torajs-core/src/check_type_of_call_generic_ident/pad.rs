//! The T-28 arity path of [`super::try_match`]: a call with fewer
//! arguments than the generic callee's parameters.

use std::collections::HashMap;

use super::arg;
use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type, substitute_typevars};
use crate::check_typevar::{typevar_appears_in_iter, unify_typevar};

/// Trailing-typevar-Any padding path. A missing slot is paddable when
/// it is a fresh TypeVar (an unannotated param — §10.2.1.4 binds it
/// undefined, so the var resolves to Any) or an explicit/materialized
/// Any (which holds the undefined box directly). A trailing TypeVar may
/// appear in the RETURN type — it just resolves the return to Any
/// (`f(x = y, y)` with `return y` answers undefined, the t262 ref-later
/// missing-arg shape) — but one bound by an EARLIER param stays a
/// strict arity error: its binding could be a concrete type whose slot
/// can't carry the undefined pad. `None` when the call does not take
/// this path.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_pad(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    name: &str,
    type_params: &[String],
    params: &[Type],
    ret: &Type,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    if args.len() >= params.len() {
        return None;
    }
    let missing = params.len() - args.len();
    let trailing = &params[args.len()..];
    let trailing_typevars: Vec<&str> = trailing
        .iter()
        .filter_map(|p| match p {
            Type::TypeVar(n) => Some(n.as_str()),
            _ => None,
        })
        .collect();
    let trailing_padable = trailing
        .iter()
        .all(|p| matches!(p, Type::TypeVar(_) | Type::Any));
    let earlier = &params[..args.len()];
    let trailing_independent = trailing_padable
        && trailing_typevars
            .iter()
            .all(|tv| !typevar_appears_in_iter(earlier, tv));
    if !trailing_independent {
        return None;
    }
    let mut subst: HashMap<String, Type> = HashMap::new();
    for (i, (param_ty, arg_id)) in params.iter().take(args.len()).zip(args.iter()).enumerate() {
        let arg_ty = match checker.type_of(ast, *arg_id) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        // an `any` argument the instantiation takes as `any` binds no
        // type variable; those left unbound pad to `any` below
        if crate::check_monomorph::any_fn_param::takes_any(param_ty, &arg_ty) {
            continue;
        }
        let arg_ty = arg::unify_view(checker, ast, param_ty, None, *arg_id, arg_ty);
        if let Err(e) = unify_typevar(param_ty, &arg_ty, &mut subst, &ast.class_parents) {
            return Some(Err(format!("argument {i} to `{name}`: {e}")));
        }
    }
    for tv in &trailing_typevars {
        subst.insert(tv.to_string(), Type::Any);
    }
    for tp in type_params {
        subst.entry(tp.clone()).or_insert(Type::Any);
    }
    let resolved_ret = substitute_typevars(ret, &subst);
    let type_args: Vec<Type> = type_params
        .iter()
        .map(|tp| subst.get(tp).cloned().unwrap())
        .collect();
    checker
        .generic_call_sites
        .insert(eid, (name.to_string(), type_args));
    checker.arity_pad_count.insert(eid, missing);
    arg::record_objlit_args(checker, ast, params, args, &subst);
    Some(Ok(resolved_ret))
}
