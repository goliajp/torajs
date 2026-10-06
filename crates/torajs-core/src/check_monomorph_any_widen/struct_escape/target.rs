//! Which functions the struct-escape lane can twin.

use crate::ast::{Ast, Param, Stmt};
use crate::check::Checker;
use crate::check_type_ann::{str_or_arr_ann, struct_shaped_ann};

/// The user params of `target` (past a lifted body's `__env` head)
/// when this lane can twin it: a non-generic, non-generator
/// declaration (an async generator's factory only with its
/// desugar-time twin) with no receiver or argv channel, declaring at
/// least one non-rest struct parameter.
pub(in crate::check_monomorph_any_widen) fn widenable_target(
    c: &Checker,
    ast: &Ast,
    target: &str,
) -> Option<Vec<Param>> {
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
    // the twin carries no hidden argc / argv head, so a body that reads
    // `arguments` mints one only for the struct slots it always served
    let headless = ast.headless_argc_fns.contains(target)
        || ast.headless_argv_fns.contains(target)
        || ast.this_first_argc_fns.contains_key(target);
    let has_struct = params.iter().any(|p| {
        !p.is_rest
            && p.type_ann.as_deref().is_some_and(|ann| {
                struct_shaped_ann(ann, &c.aliases, &c.generic_alias_decls)
                    || !headless && str_or_arr_ann(ann, &c.aliases, &c.generic_alias_decls)
            })
    });
    (plain && has_struct).then_some(params)
}
