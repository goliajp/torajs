//! Which functions the struct-escape lane can twin, and why.
//!
//! A `number` or `boolean` slot holds any value of its kind, so only a
//! call through `any` hands it another kind; the typed adapter would
//! then run ToNumber / ToBoolean on it, and the body would see a value
//! nobody passed (§10.2.1.3 binds the argument as it is). Such a slot
//! needs the twin only when the function's value can be called through
//! `any` at all, so a target that qualifies by scalar slots alone is
//! twinned only behind a loose value ([`Twinnable::needs_escape`]).

use crate::ast::{Ast, NameRead, Param, Stmt};
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

/// The parameters of the top-level declaration `target`, past a lifted
/// body's `__env` head, when its body can be cloned into an all-`any`
/// twin: not generic, not a generator (an async generator's factory
/// only with its desugar-time twin), with no receiver or argv channel.
/// With `recv`, a receiver head is admitted and kept: a lifted body's
/// (`__env` then `__this`, a promoted function expression) or a
/// promoted declaration's (`__this`, reached through its receiver-first
/// shell). The boxed entry takes the receiver in argv[0], and so does
/// the twin's, so the mono adapter hands a call over as it came.
/// Answers the same before `desugar_implicit_generics` as after it: an
/// untyped parameter of a plain declaration is a type variable to be.
/// The `any`-alias devirtualization asks it too, to keep on the `any`
/// lane only calls whose target this lane will twin.
pub(crate) fn twin_clone_params(ast: &Ast, target: &str, recv: bool) -> Option<Vec<Param>> {
    let (params, type_params, is_generator) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            type_params,
            is_generator,
            ..
        } if name == target => Some((params, type_params, *is_generator)),
        _ => None,
    })?;
    let lifted = params.first().is_some_and(|p| p.name == "__env");
    let params = params[usize::from(lifted)..].to_vec();
    let implicit_generic = !lifted && params.iter().any(|p| !p.is_rest && p.type_ann.is_none());
    let recv_head = usize::from(
        recv && params.first().is_some_and(|p| p.name == "__this")
            && if lifted {
                ast.fnexpr_recv_fns.contains(target)
            } else {
                ast.this_param_fns.contains(target)
            },
    );
    let plain = type_params.is_empty()
        && !implicit_generic
        && !is_generator
        && (!ast.async_generator_fns.contains(target)
            || crate::ast::minted_generator_any_twin(ast, target).is_some())
        && params[recv_head..].iter().all(|p| {
            !matches!(
                p.name.as_str(),
                "__env" | "__this" | "__torajs_argv" | crate::ast::GEN_ARGV_PARAM
            ) && p.name != crate::ast::new_target_param::NEW_TARGET
        });
    plain.then_some(params)
}

/// The target `target` when this lane can twin it
/// ([`twin_clone_params`]), declaring at least one non-rest struct,
/// string, array, `number` or `boolean` parameter. A body on the
/// hidden-argc tier counts only the parameters it may read: one it
/// never reads binds whatever the call hands it unseen.
pub(in crate::check_monomorph_any_widen) fn widenable_target(
    c: &Checker,
    ast: &Ast,
    target: &str,
    recv: bool,
) -> Option<Twinnable> {
    if c.generic_type_params.contains_key(target) {
        return None;
    }
    let params = twin_clone_params(ast, target, recv)?;
    // a receiver head admitted above is no user slot
    let recv_head = usize::from(params.first().is_some_and(|p| p.name == "__this"));
    // the twin carries the hidden argc head of a length-only body
    // (`run` mirrors it), but no argv head, so a body that reads
    // argument values mints one only for the struct slots it always
    // served
    let headless =
        ast.headless_argv_fns.contains(target) || ast.this_first_argc_fns.contains_key(target);
    let unseen = unseen_params(ast, target);
    let anns = || {
        params[recv_head..]
            .iter()
            .filter(|p| !p.is_rest && !unseen.contains(&p.name.as_str()))
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

/// The parameters a body on the hidden-argc tier that reads no
/// argument value never reads. Its materialized arguments local
/// (`__torajs_arguments`) lists them, but only its length is observed,
/// and that is the hidden argc.
fn unseen_params<'a>(ast: &'a Ast, target: &str) -> Vec<&'a str> {
    if !ast.headless_argc_fns.contains(target) || ast.headless_argv_fns.contains(target) {
        return Vec::new();
    }
    let Some((params, body)) = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name, params, body, ..
        } if name == target => Some((params, body)),
        _ => None,
    }) else {
        return Vec::new();
    };
    let stmts = body
        .iter()
        .filter(|s| !matches!(s, Stmt::LetDecl { name, .. } if name == "__torajs_arguments"));
    params
        .iter()
        .filter(|p| {
            stmts.clone().all(|s| {
                crate::ast::body_reads(ast, std::slice::from_ref(s), &p.name) == NameRead::No
            })
        })
        .map(|p| p.name.as_str())
        .collect()
}
