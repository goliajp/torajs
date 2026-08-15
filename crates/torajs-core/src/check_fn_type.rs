//! `resolve_type_ann` / `build_fn_type` cluster pulled out of
//! [`crate::check`] as chunk-319 of the check.rs god-file decomp.
//!
//! Five thin / medium wrappers around the heavy lifters
//! `resolve_type_ann_full` (in `check_type_ann.rs`) and the
//! local `build_fn_type_full`:
//!
//! - `resolve_type_ann` — bare-name lookup (no type vars, no generic-alias map).
//! - `resolve_type_ann_with_vars` — adds in-scope type-parameter names; dead
//!   (kept for symmetry with the `with_vars` shape; flagged `#[allow(dead_code)]`).
//! - `build_fn_type` — fn-signature → `Type::Function` (no generics).
//! - `build_fn_type_with_vars` — adds in-scope type-parameter names; dead.
//! - `build_fn_type_full` — full resolver (also accepts a generic-alias-decls
//!   map). All shorter variants funnel into this one.
//!
//! Re-exported back into `crate::check` so existing callers
//! (`check_type_of_fn`, `check_pipeline`) keep their import path.

use crate::ast::Param;
use crate::check::{GenericAliasMap, Type};
use crate::check_type_ann::resolve_type_ann_full;
use std::collections::HashMap;

#[allow(dead_code)]
pub(crate) fn resolve_type_ann(name: &str, aliases: &HashMap<String, Type>) -> Option<Type> {
    resolve_type_ann_full(name, aliases, &[], &HashMap::new())
}

/// Like `resolve_type_ann`, but accepts a slice of in-scope type-parameter
/// names. A bare identifier matching one of those resolves to
/// `Type::TypeVar(name)` regardless of any conflicting alias / primitive.
/// Used by `build_fn_type` for generic fn bodies. M3.
#[allow(dead_code)]
pub(crate) fn resolve_type_ann_with_vars(
    name: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
) -> Option<Type> {
    resolve_type_ann_full(name, aliases, type_params, &HashMap::new())
}

/// Full resolver: also accepts a generic-alias-decls map so `Pair<X|Y>`
/// instantiates against the original `type Pair<A, B> = { ... }` decl.
/// M3.4.
pub(crate) fn build_fn_type(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
) -> Result<Type, String> {
    build_fn_type_with_vars(fn_name, params, return_type, aliases, &[])
}

pub(crate) fn build_fn_type_with_vars(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
) -> Result<Type, String> {
    build_fn_type_full(
        fn_name,
        params,
        return_type,
        aliases,
        type_params,
        &HashMap::new(),
    )
}

pub(crate) fn build_fn_type_full(
    fn_name: &str,
    params: &[Param],
    return_type: &Option<String>,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
) -> Result<Type, String> {
    let mut param_tys = Vec::new();
    for p in params {
        let Some(ann) = &p.type_ann else {
            return Err(format!(
                "parameter `{}` of function `{fn_name}` requires a type annotation",
                p.name
            ));
        };
        let Some(ty) = resolve_type_ann_full(ann, aliases, type_params, generic_aliases) else {
            return Err(format!(
                "unknown type `{ann}` for parameter `{}` of function `{fn_name}`",
                p.name
            ));
        };
        // RFC 20260815-fn-value-rest-spread 刀 1 — a rest param
        // registers as the `Type::Rest(elem)` sentinel, the same
        // per-argument shape a `(...args: E[]) => R` ANNOTATION
        // resolves to, so a fn VALUE made of this decl rides the
        // variadic call machinery. Direct calls saw their trailing
        // args packed into one array literal by `apply_rest_args`
        // (name-keyed, before the checker) — the bare-global-callee
        // fold in the call arms hands those exactly the packed
        // `Array(elem)` param this used to register.
        if p.is_rest {
            let elem = match ty {
                Type::Array(e) => *e,
                other => other,
            };
            param_tys.push(Type::Rest(Box::new(elem)));
        } else {
            param_tys.push(ty);
        }
    }
    let ret_ty = match return_type {
        None => Type::Void,
        Some(t) => match resolve_type_ann_full(t, aliases, type_params, generic_aliases) {
            Some(ty) => ty,
            None => {
                return Err(format!(
                    "unknown return type `{t}` for function `{fn_name}`"
                ));
            }
        },
    };
    Ok(Type::Function(param_tys, Box::new(ret_ty)))
}
