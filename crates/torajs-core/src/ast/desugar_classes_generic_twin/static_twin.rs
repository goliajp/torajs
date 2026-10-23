//! The static-method twin `__smany_<C>__<m>`, the mirror of the
//! instance twin in the parent module.

use super::{TwinSource, neutralize_clone, restore_dynamic_calls};
use crate::ast::ast_def::Ast;
use crate::ast::clone_body::BodyCloner;
use crate::ast::desugar_classes_ctor_twin::has_lossy_param;
use crate::ast::expr::{Expr, Param};
use crate::ast::stmt::Stmt;
use crate::check_pipeline::TypeAliasTables;

/// RFC 20260804-fn-this-channel knife 3a — the STATIC-method mirror
/// of [`super::mint_generic_twin`]: mint `__smany_<C>__<m>` beside
/// `__sm_<C>__<m>` when the static body reads `this`.
///
/// A static body's `this` is the class object at a direct `C.g()`
/// call, and desugar pass 2 rewrote every such site to `Ident(<cls>)`
/// (knife 2 recorded the sites in `ast.static_this_sites`, keyed by
/// the ORIGINAL ExprId — which is exactly what lets this mint tell a
/// receiver read apart from a user-written `C.x`). The twin restores
/// the receiver at those sites: each cloned counterpart becomes
/// `__this`, declared as the leading `any` param, so `C.g.call(recv)`
/// can rebind per §27.7.5.1 — member reads lower through the any
/// lane, and a `this.#f()` rides the private-brand kernels (a
/// receiver without the brand throws, an instance with it runs).
///
/// Mint drops (clone neutralized, mono behavior kept — the recorded
/// RFC residue): a body with no receiver read (`this`-free statics
/// never rebind-misbehave), and a body carrying a static-dispatch
/// call form with no restore record (the super rewrite — see
/// [`restore_dynamic_calls`]; a static-this `this.m()` rewrite
/// records its intact Member as a speculative-demote alt, which the
/// restore step copies back so the twin dispatches dynamically).
pub(in crate::ast) fn mint_static_generic_twin(
    ast: &mut Ast,
    tables: &TypeAliasTables,
    src: TwinSource<'_>,
    appended: &mut Vec<Stmt>,
) {
    let TwinSource {
        mono_name,
        params,
        body,
        type_params,
        span,
    } = src;
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let reads_this = rebind_this_sites(&mut cloner);
    if !reads_this && !has_lossy_param(cloner.ast, tables, params, true) {
        neutralize_clone(&mut cloner);
        return;
    }
    crate::ast::clone_body_tables::migrate(&mut cloner);
    // Same restore step as the instance mint: every static-dispatch
    // call form in the clone goes back to its member-call shape (a
    // static body's `this.#f()` recorded its intact Member as a
    // speculative-demote alt — see `restore_dynamic_calls`). A form
    // with no record (the super rewrite) drops the mint.
    if !restore_dynamic_calls(&mut cloner, params, body) {
        neutralize_clone(&mut cloner);
        return;
    }
    // Rebind AGAIN, because `migrate` may have minted more receiver
    // reads: a speculative alt lives OUTSIDE the body (it hangs off
    // the side table, not off a cloned stmt), so `resolve_value`
    // deep-clones it — and `clone_expr` does not memoize, so the
    // alt's own receiver read becomes a FRESH `Ident(<cls>)` node
    // rather than the one the first pass just rebound. Since
    // `restore_dynamic_calls` installs exactly that alt shape as the
    // twin's call, skipping this second pass leaves the twin reading
    // the class object (`C.f()`) instead of its rebound receiver —
    // every `C.g.call(recv)` then missed, silently for a field read
    // and as "not a function" for a method call.
    rebind_this_sites(&mut cloner);
    let mut cloned_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
    cloned_params.push(Param {
        name: "__this".into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    });
    cloned_params.extend(params.iter().map(|p| Param {
        name: p.name.clone(),
        type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
        default: p.default.map(|d| cloner.clone_expr(d)),
        is_rest: p.is_rest,
    }));
    let twin_name = format!("__smany_{}", &mono_name["__sm_".len()..]);
    ast.smany_twins
        .insert(mono_name.to_string(), twin_name.clone());
    if reads_this {
        ast.smany_reads_this.insert(mono_name.to_string());
    }
    appended.push(Stmt::FnDecl {
        name: twin_name,
        type_params: type_params.to_vec(),
        params: cloned_params,
        return_type: Some("any".to_string()),
        body: cloned_body,
        is_generator: false,
        span,
    });
}

/// Point every cloned counterpart of a static-`this` site at the
/// twin's leading receiver param. `false` = the body reads no
/// receiver at all (a `this`-free static never rebind-misbehaves, so
/// the caller drops the mint and keeps direct-mono dispatch).
///
/// Idempotent, and deliberately re-run after
/// [`crate::ast::clone_body_tables::migrate`] — see the second call site.
fn rebind_this_sites(cloner: &mut BodyCloner<'_>) -> bool {
    let sites: Vec<crate::ast::ExprId> = cloner
        .map
        .iter()
        .filter(|(old, _)| cloner.ast.static_this_sites.contains_key(old))
        .map(|(_, new)| *new)
        .collect();
    let found = !sites.is_empty();
    for new in sites {
        cloner.ast.exprs[new.0 as usize] = Expr::Ident("__this".into());
    }
    found
}
