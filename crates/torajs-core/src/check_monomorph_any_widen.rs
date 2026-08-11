//! RFC 20260802-any-arg-typed-param-mono — retarget every call site
//! the checker admitted with an `Array(Any)` argument against a typed
//! `Array(T)` param (`check_type_of_call/general.rs` reverse-widen
//! wedge, `Checker::any_widen_mono_sites`).
//!
//! TS any-assignability says `any[]` flows into `number[]`, but tr's
//! runtime reprs differ: an Arr<Any> block holds NaN-boxed slots
//! while a typed `Array(T)` reader loads raw i64/f64 — handing the
//! block through directly reads garbage bits. Instead of taxing every
//! typed reader with a kind check, the callee is re-instantiated with
//! the mismatched params widened to `any[]` (its body re-checks and
//! re-lowers through the kind-aware any lanes) and the call site
//! retargets to the clone via the existing `call_retargets` channel.
//! Typed callers keep the original decl untouched — zero hot-path
//! cost. The original fn stays live (unlike the generic originals it
//! is lowered as-is), so its lifted closures are NOT retired; the
//! spec body gets its own closure clones per RFC 20260731.
//!
//! Alternates with the generic worklist until both drain: a spec
//! body's re-check can seed generic instantiations, and a generic
//! specialization's body check can record new any-widen sites.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{Ast, ExprId, Stmt};
use crate::check::{Checker, Type};
use crate::check_monomorph::{
    WorkItem, clone_spec_body, process_one_generic, seed_records, spec_params_env,
};
use crate::ssa_lower_generics_monomorph::Generics;

/// Fixpoint driver — called by `monomorphize_and_check` after its
/// first generic drain. Every `any_widen_mono_sites` entry ends up in
/// `call_retargets` (the admit gate guarantees a clonable FnDecl).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    generic_fn_names: &mut HashSet<String>,
) {
    loop {
        while let Some(item) = worklist.pop_front() {
            process_one_generic(
                c,
                owned_ast,
                generics,
                cache,
                worklist,
                call_retargets,
                mono_decls,
                generic_fn_names,
                item,
            );
        }
        let moved = widen_one(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
        ) || escape_widen_one(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
        );
        if !moved && worklist.is_empty() {
            break;
        }
    }
}

/// L3b ③ twin of [`widen_one`] for the VALUE-escape sites
/// (`Checker::fn_escape_widen_sites`): clone the fn with EVERY
/// TypeVar param widened to plain `any` (suffix `$$anywv`) and
/// retarget the escaping init Ident to the clone through the same
/// `call_retargets` channel the ident lowering already consults for
/// ExprId-keyed renames. Returns false when no site is pending.
fn escape_widen_one(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
) -> bool {
    let mut pending: Vec<(ExprId, String)> = c
        .fn_escape_widen_sites
        .iter()
        .filter(|(eid, _)| !call_retargets.contains_key(eid))
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    pending.sort_by_key(|(eid, _)| eid.0);
    let Some((init_eid, fn_name)) = pending.into_iter().next() else {
        return false;
    };
    // Widen every TypeVar-typed param (the implicit-generics pass
    // types each unannotated param with its own fresh TypeVar).
    let idxs: Vec<usize> = match c.globals.get(&fn_name) {
        Some(Type::Function(ptys, _)) => ptys
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t, Type::TypeVar(_)))
            .map(|(i, _)| i)
            .collect(),
        _ => Vec::new(),
    };
    let mono_name = format!("{fn_name}$$anywv");
    if owned_ast.headless_argc_fns.contains(&fn_name) {
        owned_ast.headless_argc_fns.insert(mono_name.clone());
    }
    if !c.globals.contains_key(&mono_name) {
        emit_widened_spec(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            &fn_name,
            &mono_name,
            "$$anywv",
            &idxs,
            WidenTarget::Scalar,
        );
    }
    call_retargets.insert(init_eid, mono_name);
    true
}

/// What a widened param slot becomes: the array channel rewrites
/// `Array(Any)`-admitted params to `any[]`; the value-escape channel
/// rewrites TypeVar params to plain `any`.
enum WidenTarget {
    Arr,
    Scalar,
}

impl WidenTarget {
    fn ann(&self) -> &'static str {
        match self {
            WidenTarget::Arr => "any[]",
            WidenTarget::Scalar => "any",
        }
    }
    fn ty(&self) -> Type {
        match self {
            WidenTarget::Arr => Type::Array(Box::new(Type::Any)),
            WidenTarget::Scalar => Type::Any,
        }
    }
}

/// Clone + check + retarget the lowest-ExprId pending site (lowest
/// first for deterministic mono-decl emission). Returns false when no
/// site is pending.
fn widen_one(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
) -> bool {
    let mut pending: Vec<(ExprId, (String, Vec<usize>))> = c
        .any_widen_mono_sites
        .iter()
        .filter(|(eid, _)| !call_retargets.contains_key(eid))
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    pending.sort_by_key(|(eid, _)| eid.0);
    let Some((call_eid, (fn_name, mut idxs))) = pending.into_iter().next() else {
        return false;
    };
    idxs.sort_unstable();
    idxs.dedup();
    let suffix = format!(
        "$$anyw{}",
        idxs.iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("_")
    );
    let mono_name = format!("{fn_name}{suffix}");
    // H1 — mirror a head-less body's any-widened clone into the
    // hidden-argc side table (same contract as the generic channel's
    // mirror in `process_one_generic`).
    if owned_ast.headless_argc_fns.contains(&fn_name) {
        owned_ast.headless_argc_fns.insert(mono_name.clone());
    }
    if !c.globals.contains_key(&mono_name) {
        emit_widened_spec(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            &fn_name,
            &mono_name,
            &suffix,
            &idxs,
            WidenTarget::Arr,
        );
    }
    call_retargets.insert(call_eid, mono_name);
    true
}

/// Materialize the widened specialization: deep-clone the original
/// decl's body (fresh ExprIds), rewrite the widened params' anns to
/// `any[]`, register the signature, clone referenced lifted closures,
/// check the body (diagnostics suppressed — same waterline stance as
/// the generic channel), and seed whatever the check recorded.
#[allow(clippy::too_many_arguments)]
fn emit_widened_spec(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    fn_name: &str,
    mono_name: &str,
    suffix: &str,
    idxs: &[usize],
    target: WidenTarget,
) {
    let Some((params, return_type, body)) = owned_ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } if name == fn_name => Some((params.clone(), return_type.clone(), body.clone())),
        _ => None,
    }) else {
        // The admit gate verified the decl exists; an absent decl here
        // would leave the site retargeted to an unknown name (loud at
        // lower), never a silent wrong block.
        return;
    };
    let no_subst: Vec<(String, String)> = Vec::new();
    let (mut new_params, new_return_type, new_body, id_map) =
        clone_spec_body(owned_ast, &params, &return_type, &body, &no_subst);
    for &i in idxs {
        if let Some(p) = new_params.get_mut(i) {
            p.type_ann = Some(target.ann().to_string());
        }
    }
    // Value-escape clone: a TypeVar return ANN (the AST-side `__T`)
    // erases to `any` too — the spec has no type params to bind it.
    let new_return_type = match (&target, &new_return_type) {
        (WidenTarget::Scalar, Some(rt))
            if c.generic_type_params
                .get(fn_name)
                .is_some_and(|tvs| tvs.contains(rt)) =>
        {
            Some("any".to_string())
        }
        _ => new_return_type,
    };
    if let Some(Type::Function(ptys, rty)) = c.globals.get(fn_name).cloned() {
        let mut spec_ptys = ptys;
        for &i in idxs {
            if let Some(t) = spec_ptys.get_mut(i) {
                *t = target.ty();
            }
        }
        // The value-escape channel also erases a TypeVar RETURN (the
        // clone's body re-infers under any params; a leftover TypeVar
        // face would re-reject the binding's call).
        let rty = if matches!(target, WidenTarget::Scalar) && matches!(*rty, Type::TypeVar(_)) {
            Box::new(Type::Any)
        } else {
            rty
        };
        c.globals
            .insert(mono_name.to_string(), Type::Function(spec_ptys, rty));
    }
    if let Some(d) = c.fn_defaults.get(fn_name).cloned() {
        c.fn_defaults.insert(mono_name.to_string(), d);
    }
    let spec_decl = Stmt::FnDecl {
        name: mono_name.to_string(),
        type_params: Vec::new(),
        params: new_params,
        return_type: new_return_type,
        body: new_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    };
    let saved_sites = std::mem::take(&mut c.generic_call_sites);
    let saved_pads = std::mem::take(&mut c.arity_pad_count);
    let saved_error_count = c.errors.len();
    // Spec-local closure clones (RFC 20260731 channel). The rename
    // keys are NOT retired into `generic_fn_names`: the original fn
    // still lowers, so its closures' original construction sites stay
    // live — retiring them would break the typed callers.
    let _closure_rename = crate::check_monomorph_closures::clone_spec_closures(
        c, owned_ast, suffix, &id_map, &no_subst,
    );
    c.check_stmt(owned_ast, &spec_decl);
    c.errors.truncate(saved_error_count);
    let inner_sites = std::mem::replace(&mut c.generic_call_sites, saved_sites);
    let inner_pads = std::mem::replace(&mut c.arity_pad_count, saved_pads);
    let mut inner: Vec<(ExprId, (String, Vec<Type>))> =
        inner_sites.iter().map(|(k, v)| (*k, v.clone())).collect();
    inner.sort_by_key(|(eid, _)| eid.0);
    let param_env: HashMap<String, String> = spec_params_env(&spec_decl);
    seed_records(
        owned_ast,
        &inner,
        generics,
        &param_env,
        cache,
        worklist,
        call_retargets,
    );
    c.generic_call_sites.extend(inner_sites);
    c.arity_pad_count.extend(inner_pads);
    mono_decls.push(spec_decl);
}
