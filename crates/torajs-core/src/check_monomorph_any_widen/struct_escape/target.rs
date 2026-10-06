//! Which functions the struct-escape lane can twin, and why.
//!
//! A `number` or `boolean` slot holds any value of its kind, so only a
//! call through `any` hands it another kind; the typed adapter would
//! then run ToNumber / ToBoolean on it, and the body would see a value
//! nobody passed (§10.2.1.3 binds the argument as it is). Such a slot
//! needs the twin only when the function's value can be called through
//! `any` at all, so a target that qualifies by scalar slots alone is
//! twinned only behind a loose value ([`Twinnable::needs_escape`]).

use crate::ast::{Ast, Param, Stmt};
use crate::check::{Checker, Type};
use crate::check_type_ann::{resolve_type_ann_full, str_or_arr_ann, struct_shaped_ann};

/// A target the lane can twin.
pub(in crate::check_monomorph_any_widen) struct Twinnable {
    /// The user params, past a lifted body's `__env` head.
    pub(in crate::check_monomorph_any_widen) params: Vec<Param>,
    /// Only `number` / `boolean` slots qualify it: twin it only behind
    /// a value that may be called through `any`.
    pub(in crate::check_monomorph_any_widen) needs_escape: bool,
}

/// The target `target` when this lane can twin it: a non-generic,
/// non-generator declaration (an async generator's factory only with
/// its desugar-time twin) with no receiver or argv channel, declaring
/// at least one non-rest struct, string, array, `number` or `boolean`
/// parameter.
pub(in crate::check_monomorph_any_widen) fn widenable_target(
    c: &Checker,
    ast: &Ast,
    target: &str,
) -> Option<Twinnable> {
    let (params, type_params, is_generator) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            type_params,
            is_generator,
            ..
        } if name == target => Some((params.clone(), type_params, *is_generator)),
        _ => None,
    })?;
    let params = params[usize::from(params.first().is_some_and(|p| p.name == "__env"))..].to_vec();
    let plain = type_params.is_empty()
        && !is_generator
        && !c.generic_type_params.contains_key(target)
        && (!ast.async_generator_fns.contains(target)
            || crate::ast::minted_generator_any_twin(ast, target).is_some())
        && params.iter().all(|p| {
            !matches!(
                p.name.as_str(),
                "__env" | "__this" | "__torajs_argv" | crate::ast::GEN_ARGV_PARAM
            ) && p.name != crate::ast::new_target_param::NEW_TARGET
        });
    if !plain {
        return None;
    }
    // the twin carries no hidden argc / argv head, so a body that reads
    // `arguments` mints one only for the struct slots it always served
    let headless = ast.headless_argc_fns.contains(target)
        || ast.headless_argv_fns.contains(target)
        || ast.this_first_argc_fns.contains_key(target);
    let anns = || {
        params
            .iter()
            .filter(|p| !p.is_rest)
            .filter_map(|p| p.type_ann.as_deref())
    };
    let shaped = anns().any(|ann| {
        struct_shaped_ann(ann, &c.aliases, &c.generic_alias_decls)
            || !headless && str_or_arr_ann(ann, &c.aliases, &c.generic_alias_decls)
    });
    let scalar = !headless
        && anns().any(|ann| {
            matches!(
                resolve_type_ann_full(ann, &c.aliases, &[], &c.generic_alias_decls),
                Some(Type::Number | Type::Boolean)
            )
        });
    (shaped || scalar).then_some(Twinnable {
        params,
        needs_escape: !shaped,
    })
}
