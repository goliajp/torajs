//! M3 — generic call inference for bare-Ident callee naming a
//! generic FnDecl, extracted from
//! [`crate::check_type_of_call::check`]'s top-level cascade
//! (chunk 219 — thirteenth sub-batch of check_type_of_call.rs
//! per-shape decomposition).
//!
//! If `callee` is an `Expr::Ident(name)` and `name` resolves to
//! a generic `FnDecl` (recorded in `checker.generic_type_params`
//! and `checker.globals`), walk param/arg pairs unifying each
//! TypeVar against the actual arg type, then substitute back
//! into the return type. The inferred substitution is recorded
//! into `checker.generic_call_sites` (keyed by the call's
//! `ExprId`) so ssa_lower can monomorphize.
//!
//! Two arity paths:
//!
//! - **T-28** — `args.len() < params.len()`. Default param
//!   missing → undefined for implicit-generic fns. Untyped JS
//!   params (`function f(a, b)`) get rewritten to fresh
//!   independent TypeVars by `desugar_implicit_generics`;
//!   trailing missing params that are all TypeVar AND not
//!   referenced earlier or in the return type get bound to
//!   `Type::Any` and padded with `ANY_UNDEF` at the call site.
//!   `checker.arity_pad_count` records the missing count so
//!   ssa_lower can pad.
//!
//! - **regular** — `args.len() == params.len()`. Unify each
//!   pair, validate every type-param was bound, substitute
//!   into the return type.
//!
//! Returns `Some(Ok(resolved_ret))` on match; `Some(Err(_))`
//! on unification / arity / arg typecheck failure; `None`
//! when callee isn't a bare-Ident generic-fn reference (other
//! callee shapes fall through to the regular dispatch path).

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type, substitute_typevars};
use crate::check_typevar::{typevar_appears_in, typevar_appears_in_iter, unify_typevar};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    if let Expr::Ident(name) = ast.get_expr(*callee)
        && let Some(type_params) = checker.generic_type_params.get(name).cloned()
        && let Some(Type::Function(params, ret)) = checker.globals.get(name).cloned()
    {
        // T-28 — trailing-typevar-Any padding path.
        if args.len() < params.len() {
            let missing = params.len() - args.len();
            let trailing = &params[args.len()..];
            let trailing_typevars: Vec<&str> = trailing
                .iter()
                .filter_map(|p| match p {
                    Type::TypeVar(n) => Some(n.as_str()),
                    _ => None,
                })
                .collect();
            let trailing_all_typevar = trailing_typevars.len() == trailing.len();
            let earlier = &params[..args.len()];
            let trailing_independent = trailing_all_typevar
                && trailing_typevars.iter().all(|tv| {
                    !typevar_appears_in_iter(earlier, tv) && !typevar_appears_in(&ret, tv)
                });
            if trailing_independent {
                let mut subst: HashMap<String, Type> = HashMap::new();
                for (i, (param_ty, arg_id)) in
                    params.iter().take(args.len()).zip(args.iter()).enumerate()
                {
                    let arg_ty = match checker.type_of(ast, *arg_id) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    if let Err(e) = unify_typevar(param_ty, &arg_ty, &mut subst) {
                        return Some(Err(format!("argument {i} to `{name}`: {e}")));
                    }
                }
                for tv in &trailing_typevars {
                    subst.insert(tv.to_string(), Type::Any);
                }
                for tp in &type_params {
                    subst.entry(tp.clone()).or_insert(Type::Any);
                }
                let resolved_ret = substitute_typevars(&ret, &subst);
                let type_args: Vec<Type> = type_params
                    .iter()
                    .map(|tp| subst.get(tp).cloned().unwrap())
                    .collect();
                checker
                    .generic_call_sites
                    .insert(eid, (name.clone(), type_args));
                checker.arity_pad_count.insert(eid, missing);
                return Some(Ok(resolved_ret));
            }
        }
        // Regular path — params.len() == args.len() required.
        if params.len() != args.len() {
            return Some(Err(format!(
                "expected {} argument(s) to `{name}`, got {}",
                params.len(),
                args.len()
            )));
        }
        let mut subst: HashMap<String, Type> = HashMap::new();
        let mut arg_tys: Vec<Type> = Vec::with_capacity(args.len());
        for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
            let arg_ty = match checker.type_of(ast, *arg_id) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            if let Err(e) = unify_typevar(param_ty, &arg_ty, &mut subst) {
                return Some(Err(format!("argument {i} to `{name}`: {e}")));
            }
            arg_tys.push(arg_ty);
        }
        // Validate every type-param was bound.
        for tp in &type_params {
            if !subst.contains_key(tp) {
                return Some(Err(format!(
                    "could not infer type parameter `{tp}` for `{name}`"
                )));
            }
        }
        let resolved_ret = substitute_typevars(&ret, &subst);
        // Record the substitution for the SSA monomorphizer.
        // Keyed by ExprId of the call so each call site gets
        // its own type-argument set.
        let type_args: Vec<Type> = type_params
            .iter()
            .map(|tp| subst.get(tp).cloned().unwrap())
            .collect();
        checker
            .generic_call_sites
            .insert(eid, (name.clone(), type_args));
        // Generic call args also follow the new TS-shape
        // borrow semantics — non-Copy args are not consumed
        // by passing. See the comment in the regular Call
        // arm in `check_type_of_call` for the rationale +
        // caveat.
        let _ = params;
        let _ = args;
        return Some(Ok(resolved_ret));
    }
    None
}
