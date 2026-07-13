//! Post-check monomorphization — specializations are emitted AND
//! type-checked here, at the end of the check pipeline, instead of
//! inside ssa_lower (RFC 20260713-mono-check-specializations).
//!
//! Why: checker pass-2 skips generic FnDecl bodies (TypeVar-bearing,
//! can't check without substitution), so a generic call INSIDE a
//! generic body had no `generic_call_sites` record. The old lower-
//! side monomorphizer patched that hole with
//! `rewrite_inner_generic_calls`' same-name-type_params heuristic —
//! blind-reusing the caller's subst. Correct for class members
//! sharing the class's type params, but two INDEPENDENT implicit-
//! generic fns also collide on the fresh `__T1` name
//! (`desugar_implicit_generics` numbers per fn), instantiating the
//! callee at the CALLER's type args regardless of the actual
//! arguments (an I64 arg consumed as Str → SIGSEGV; test262
//! special_casing_Lithuanian charInfo/hexString). Distinct-name
//! typevars fared no better: nothing rewrote the callee at all
//! ("unknown function").
//!
//! The orthodox fix is the C++ two-phase-lookup / Rust model:
//! instantiate, THEN check the concrete body. Each specialization
//! emitted off the worklist is run through `check_stmt` — its body
//! is fully substituted, so the checker's own call-site inference
//! records every inner generic call (fresh clone ExprIds, concrete
//! type args) and those records seed further instantiations until
//! the worklist converges. `expr_types` / `arity_pad_count` entries
//! for the clone land in the main tables directly; the per-body
//! generic-call records are swap-captured so this pass can seed
//! retargets from exactly the new entries.
//!
//! The demoted-cm restore (see `cm_demote.rs` hop 3) moves here from
//! ssa_lower — it must still run before cloning so specialization
//! bodies inherit the restored member-call shape. Speculative
//! rewrites whose decision would only be reachable inside a generic
//! body remain undecided (clone ExprIds aren't in
//! `speculative_cm_rewrites`); that face never worked and is
//! unchanged — recorded in the RFC.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::{Checker, Type, substitute_typevars};
use crate::ssa_lower_generics_monomorph::{
    Generics, collect_generics, compute_arg_anns, substitute_in_ann, substitute_in_stmt,
};

/// Monomorphization products carried to the lowerer through
/// `CheckArtifacts`. `mono_ast` is the owned post-restore AST with
/// every specialization FnDecl appended; `call_retargets` maps each
/// generic call's ExprId (top-level AND inside specialization
/// bodies) to its mono fn name; `generic_fn_names` are the original
/// generic decls lower pass-1 must skip.
pub struct MonoOutput {
    pub mono_ast: Ast,
    pub call_retargets: HashMap<ExprId, String>,
    pub generic_fn_names: HashSet<String>,
}

/// Worklist item: mono cache key (name + lowered arg anns) plus the
/// checker-level type args the specialization binds its params to.
/// Two ann keys can share one type-arg vector (I64 "number" vs
/// "f64" both check as `Type::Number`) — each still emits and
/// checks its own specialization because the clone ExprIds differ.
type WorkItem = (String, Vec<String>, Vec<Type>);

pub(crate) fn monomorphize_and_check(c: &mut Checker, ast: &Ast) -> MonoOutput {
    let mut owned_ast: Ast = ast.clone();
    // Restore the member-call shape at demoted speculative rewrites
    // BEFORE cloning, so specialization bodies and num_width see the
    // builtin dispatch shape (mechanism: cm_demote.rs).
    for (&call_eid, &alt_eid) in &c.demoted_cm_rewrites {
        owned_ast.exprs[call_eid.0 as usize] = owned_ast.exprs[alt_eid.0 as usize].clone();
    }
    let generics = collect_generics(&owned_ast);
    let generic_fn_names: HashSet<String> = generics.keys().cloned().collect();

    let mut cache: HashMap<(String, Vec<String>), String> = HashMap::new();
    let mut worklist: VecDeque<WorkItem> = VecDeque::new();
    let mut call_retargets: HashMap<ExprId, String> = HashMap::new();

    // Seed from the checker's top-level records. Sorted by ExprId so
    // the worklist (and thus mono-decl emission) is deterministic —
    // the source map iterates in hash order.
    let mut seed: Vec<(ExprId, (String, Vec<Type>))> = c
        .generic_call_sites
        .iter()
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    seed.sort_by_key(|(eid, _)| eid.0);
    // Top-level call sites live outside any specialization — empty
    // param env; f64/closure-shape hints come from the literal arg
    // expressions alone.
    let empty_env: HashMap<String, String> = HashMap::new();
    seed_records(
        &owned_ast,
        &seed,
        &generics,
        &empty_env,
        &mut cache,
        &mut worklist,
        &mut call_retargets,
    );

    let mut mono_decls: Vec<Stmt> = Vec::new();
    while let Some((name, arg_anns, type_args)) = worklist.pop_front() {
        let Some((type_params, params, return_type, body)) = generics.get(&name) else {
            continue;
        };
        let mono_name = cache[&(name.clone(), arg_anns.clone())].clone();
        // Ann-level substitution — the concrete signature the lowerer
        // parses (keeps width/closure-shape anns like "f64"/"__cls(").
        let subst: Vec<(String, String)> = type_params
            .iter()
            .cloned()
            .zip(arg_anns.iter().cloned())
            .collect();
        let mut new_params: Vec<Param> = params.clone();
        for p in new_params.iter_mut() {
            if let Some(ann) = &mut p.type_ann {
                *ann = substitute_in_ann(ann, &subst);
            }
        }
        let new_return_type = return_type.as_ref().map(|rt| substitute_in_ann(rt, &subst));
        let mut id_map: Vec<(ExprId, ExprId)> = Vec::new();
        let mut new_body: Vec<Stmt> = body
            .iter()
            .map(|s| crate::ssa_lower::deep_clone_stmt(&mut owned_ast, &mut id_map, s))
            .collect();
        for s in new_body.iter_mut() {
            substitute_in_stmt(s, &subst);
        }
        for s in new_body.iter() {
            crate::ssa_lower_generics_tvdefault::rewrite_tvdefault_in_stmt(
                &mut owned_ast,
                s,
                &subst,
            );
        }
        // Type-level signature for the checker: substitute the
        // ORIGINAL generic signature's typevars with the inferred
        // type args and register under the mono name so
        // check_stmt_fn_decl binds params to concrete types (no
        // ann-string round trip — "f64"/"__cls(" aren't checker
        // vocabulary).
        if let Some(Type::Function(ptys, rty)) = c.globals.get(&name).cloned() {
            let ty_subst: HashMap<String, Type> = type_params
                .iter()
                .cloned()
                .zip(type_args.iter().cloned())
                .collect();
            let spec_ptys: Vec<Type> = ptys
                .iter()
                .map(|t| substitute_typevars(t, &ty_subst))
                .collect();
            let spec_rty = substitute_typevars(&rty, &ty_subst);
            c.globals.insert(
                mono_name.clone(),
                Type::Function(spec_ptys, Box::new(spec_rty)),
            );
        }
        let spec_decl = Stmt::FnDecl {
            name: mono_name,
            type_params: Vec::new(),
            params: new_params,
            return_type: new_return_type,
            body: new_body,
            is_generator: false,
        };
        // Check the concrete body — INFERENCE-ONLY. Swap-capture the
        // per-ExprId tables so the inner generic calls this body
        // contains can be seeded from exactly the new records, then
        // fold them into the main tables (fresh clone ExprIds — no
        // collisions).
        //
        // Diagnostics raised inside the specialization are discarded:
        // these bodies were NEVER checked before this pass existed
        // (pass-2 skips generic decls), and the checker is stricter
        // than the runtime on shapes the lowerer supports — gate
        // fixtures caught two on the first run: `substitute_typevars`
        // leaves TypeVars inside fn-type param anns un-replaced
        // (check-generic-variadic-fn-ann-001, "expected TypeVar(T)"),
        // and closure-argc ABI arity mismatches are legal JS the
        // checker rejects (closure-argc-hof-001). Suppressing keeps
        // the error surface at the pre-RFC waterline while the
        // inference records (this pass's whole purpose) still land.
        // Promoting specializations to full strictness is recorded in
        // the RFC as a follow-up gated on those checker gaps.
        let saved_sites = std::mem::take(&mut c.generic_call_sites);
        let saved_pads = std::mem::take(&mut c.arity_pad_count);
        let saved_error_count = c.errors.len();
        c.check_stmt(&owned_ast, &spec_decl);
        c.errors.truncate(saved_error_count);
        let inner_sites = std::mem::replace(&mut c.generic_call_sites, saved_sites);
        let inner_pads = std::mem::replace(&mut c.arity_pad_count, saved_pads);
        let mut inner: Vec<(ExprId, (String, Vec<Type>))> =
            inner_sites.iter().map(|(k, v)| (*k, v.clone())).collect();
        inner.sort_by_key(|(eid, _)| eid.0);
        // Param env: this specialization's substituted param anns, so
        // an inner call passing a param straight through keeps its
        // width ("f64") and closure shape ("__cls(") — the Type layer
        // collapses both to Number/Function and a syntactic walk of
        // the arg (a bare Ident) sees neither.
        let param_env: HashMap<String, String> = spec_params_env(&spec_decl);
        seed_records(
            &owned_ast,
            &inner,
            &generics,
            &param_env,
            &mut cache,
            &mut worklist,
            &mut call_retargets,
        );
        c.generic_call_sites.extend(inner_sites);
        c.arity_pad_count.extend(inner_pads);
        mono_decls.push(spec_decl);
    }
    owned_ast.stmts.extend(mono_decls);
    MonoOutput {
        mono_ast: owned_ast,
        call_retargets,
        generic_fn_names,
    }
}

/// Resolve each recorded generic call to its mono name: derive the
/// width/closure-shape-aware ann key, reserve the mono name in the
/// shared cache (queueing the instantiation on first sight — the
/// early reservation breaks recursion cycles), and retarget the
/// call's ExprId.
fn seed_records(
    owned_ast: &Ast,
    records: &[(ExprId, (String, Vec<Type>))],
    generics: &Generics,
    param_env: &HashMap<String, String>,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
) {
    for (eid, (name, type_args)) in records {
        if !generics.contains_key(name) {
            continue;
        }
        let arg_anns = compute_arg_anns(owned_ast, *eid, name, type_args, generics, param_env);
        let cache_key = (name.clone(), arg_anns);
        let mono_name = if let Some(n) = cache.get(&cache_key) {
            n.clone()
        } else {
            let suffix: Vec<String> = cache_key
                .1
                .iter()
                .map(|a| crate::ssa_lower_generics_monomorph::name_safe(a))
                .collect();
            let mono_name = format!("{}$$_{}", name, suffix.join("_"));
            cache.insert(cache_key.clone(), mono_name.clone());
            worklist.push_back((cache_key.0, cache_key.1, type_args.clone()));
            mono_name
        };
        call_retargets.insert(*eid, mono_name);
    }
}

/// The (param name → substituted ann) environment of one emitted
/// specialization — the seed pass hands it to the width/closure-shape
/// classifiers so param-passthrough args keep their lane.
fn spec_params_env(spec_decl: &Stmt) -> HashMap<String, String> {
    let Stmt::FnDecl { params, .. } = spec_decl else {
        return HashMap::new();
    };
    params
        .iter()
        .filter_map(|p| p.type_ann.as_ref().map(|a| (p.name.clone(), a.clone())))
        .collect()
}
