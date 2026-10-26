//! The shell twin of a generic declaration with an object parameter.
//!
//! `function call<T>(o: J, t: T)` reached through `any` (`const f: any
//! = call; f(v, 0)`) enters its relay shell, whose boxed adapter unboxes
//! `v` into the instantiation's `o` slot, laid out as `J`. A class
//! instance, an object literal made in the `any` world or no object at
//! all is no such cell. The lane's other targets take an all-`any`
//! clone of the target behind the twin; a generic is instantiated by
//! its calls instead, so its twin forwards to the generic itself, and
//! that call, every argument `any`, names the instantiation that takes
//! each object parameter as `any` (`check_monomorph::any_fn_param`).

use std::collections::{HashMap, VecDeque};

use super::{construction_site, mint_shell_twin, twin_name};
use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::Checker;
use crate::check_monomorph::{WorkItem, seed_records};
use crate::check_type_ann::struct_shaped_ann;
use crate::ssa_lower_generics_monomorph::Generics;

/// Mint the shell twin of `shell` over the generic `target` when the
/// target declares an object parameter, and seed the instantiation its
/// forwarding call names.
pub(super) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    (generics, cache): (&Generics, &mut HashMap<(String, Vec<String>), String>),
    (worklist, call_retargets): (&mut VecDeque<WorkItem>, &mut HashMap<ExprId, String>),
    shell: &str,
    target: &str,
) {
    let Some(params) = params(c, owned_ast, target) else {
        return;
    };
    let Some(site) = construction_site(owned_ast, shell) else {
        return;
    };
    let saved = std::mem::take(&mut c.generic_call_sites);
    mint_shell_twin(c, owned_ast, shell, &params, target, site);
    let inner = std::mem::replace(&mut c.generic_call_sites, saved);
    if !c.globals.contains_key(&twin_name(shell)) {
        return;
    }
    let mut records: Vec<(ExprId, (String, Vec<crate::check::Type>))> =
        inner.iter().map(|(k, v)| (*k, v.clone())).collect();
    records.sort_by_key(|(eid, _)| eid.0);
    let no_env = HashMap::new();
    seed_records(
        owned_ast,
        &records,
        generics,
        &no_env,
        c,
        cache,
        worklist,
        call_retargets,
    );
    c.generic_call_sites.extend(inner);
}

/// The parameters of the generic declaration `target`, when one of them
/// is an object and the shell forwards them all: no receiver or argv
/// channel, not a generator or an async body.
fn params(c: &Checker, ast: &Ast, target: &str) -> Option<Vec<Param>> {
    let (params, is_generator) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            is_generator,
            ..
        } if name == target => Some((params, *is_generator)),
        _ => None,
    })?;
    let plain = !is_generator
        && !ast.async_fns.contains(target)
        && params.iter().all(|p| {
            !matches!(
                p.name.as_str(),
                "__env" | "__this" | "__torajs_argv" | crate::ast::GEN_ARGV_PARAM
            ) && p.name != crate::ast::new_target_param::NEW_TARGET
        });
    let object = params.iter().any(|p| {
        !p.is_rest
            && (p.type_ann.as_deref())
                .is_some_and(|a| struct_shaped_ann(a, &c.aliases, &c.generic_alias_decls))
    });
    (plain && object).then(|| params.clone())
}
