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

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{Checker, Type, substitute_typevars};
use crate::ssa_lower_generics_monomorph::{
    Generics, collect_generics, compute_arg_anns, substitute_in_ann, substitute_in_stmt,
};

/// Monomorphization products carried to the lowerer through
/// `CheckArtifacts`. `mono_ast` is the owned post-restore AST with
/// every specialization FnDecl appended; `call_retargets` maps each
/// generic call's ExprId (top-level AND inside specialization
/// bodies) to its mono fn name — plus each VALUE-escaping init
/// Ident's ExprId to its `$$anywv` clone (L3b ③; the ident lowering
/// reads the same map); `generic_fn_names` are the original
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
pub(crate) type WorkItem = (String, Vec<String>, Vec<Type>);

pub(crate) fn monomorphize_and_check(c: &mut Checker, ast: &Ast) -> MonoOutput {
    let mut owned_ast: Ast = ast.clone();
    // Restore the member-call shape at demoted speculative rewrites
    // BEFORE cloning, so specialization bodies and num_width see the
    // builtin dispatch shape (mechanism: cm_demote.rs).
    let demoted: Vec<(ExprId, ExprId)> = c
        .demoted_cm_rewrites
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    for (call_eid, alt_eid) in demoted {
        owned_ast.exprs[call_eid.0 as usize] = owned_ast.exprs[alt_eid.0 as usize].clone();
        // The demoted re-check typed the ALT node, so its T-28
        // trailing-Any pad count landed on alt_eid — but lowering
        // consults the CALL ExprId it restored the shape at. Carry
        // it over, or a demoted `d.m()` with missing any-params
        // reads garbage registers (the struct-method pad miss).
        if let Some(n) = c.arity_pad_count.get(&alt_eid).copied() {
            c.arity_pad_count.insert(call_eid, n);
        }
    }
    let generics = collect_generics(&owned_ast);
    // Also collects the original lifted-closure names whose per-spec
    // clones replace them (RFC 20260731-mono-closure-clone) — lower
    // skips those decls exactly like the generic originals: their only
    // construction sites lived in generic bodies that never lower.
    let mut generic_fn_names: HashSet<String> = generics.keys().cloned().collect();

    let mut cache: HashMap<(String, Vec<String>), String> = HashMap::new();
    let mut worklist: VecDeque<WorkItem> = VecDeque::new();
    let mut call_retargets: HashMap<ExprId, String> = HashMap::new();

    // 402-01 — pre-seed an all-`any` instance (`$$anywv`) for every
    // generic CLASS-METHOD body. Mono otherwise fires only on typed
    // call records, and the any-lane method registry / static reify
    // have no call site at all — so a generic method on an
    // `any`-held instance (`const c: any = new C(); c.id(9)`) had no
    // row to dispatch through (runtime TypeError). Non-generic
    // method bodies always get a boxed adapter (collect_boxed_targets
    // has no gate), so the all-any clone is parity, not a new tax.
    // Claimed BEFORE the call-site seeds so a later all-`any` call
    // record reuses this instance (same cache key) instead of
    // minting a `$$_any`-suffixed twin the registry sides don't
    // recognize (they strip / fall back on `$$anywv` only:
    // `collect_own_class_methods`, `try_lower_static_method_reify`).
    // `__cmany_` twins seed too (404-01): a GENERIC class's method
    // rows dispatch through the twin-any instance — the mono body
    // reads fields at one specialization's offsets, while the twin
    // reads through GetV honoring the receiver row's per-field tags.
    // `__smany_` stays out: statics bind no receiver row.
    // (`__cmany_` is its own spelling — it does NOT match the
    // `__cm_` prefix, the fifth byte differs.)
    let mut method_generics: Vec<String> = generics
        .keys()
        .filter(|n| n.starts_with("__cm_") || n.starts_with("__sm_") || n.starts_with("__cmany_"))
        .cloned()
        .collect();
    method_generics.sort();
    for name in method_generics {
        let Some((type_params, _, _, _)) = generics.get(&name) else {
            continue;
        };
        let arg_anns = vec!["any".to_string(); type_params.len()];
        let cache_key = (name.clone(), arg_anns.clone());
        if cache.contains_key(&cache_key) {
            continue;
        }
        let type_args = vec![Type::Any; type_params.len()];
        cache.insert(cache_key, format!("{name}$$anywv"));
        worklist.push_back((name, arg_anns, type_args));
    }

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
    while let Some(item) = worklist.pop_front() {
        process_one_generic(
            c,
            &mut owned_ast,
            &generics,
            &mut cache,
            &mut worklist,
            &mut call_retargets,
            &mut mono_decls,
            &mut generic_fn_names,
            item,
        );
    }
    // RFC 20260802-any-arg-typed-param-mono — retarget every admitted
    // `Array(Any)`-into-`Array(T)` call site to an any-widened callee
    // clone; alternates with the generic worklist until both drain.
    crate::check_monomorph_any_widen::run(
        c,
        &mut owned_ast,
        &generics,
        &mut cache,
        &mut worklist,
        &mut call_retargets,
        &mut mono_decls,
        &mut generic_fn_names,
    );
    owned_ast.stmts.extend(mono_decls);
    // Blade 3 — the checker's per-group lane verdicts (top-level ones
    // from the main pipeline, specialization ones from the body checks
    // above) ride to the lowerer on the AST it already reads.
    owned_ast.iter_destr_srcs = std::mem::take(&mut c.iter_destr_srcs);
    owned_ast.undeclared_reads = std::mem::take(&mut c.undeclared_reads);
    owned_ast.self_name_writes = std::mem::take(&mut c.self_name_writes);
    prune_unresolved_captures(c, &mut owned_ast);
    MonoOutput {
        mono_ast: owned_ast,
        call_retargets,
        generic_fn_names,
    }
}

/// One generic-worklist item: emit + check the specialization for
/// `(name, arg_anns, type_args)` and seed whatever its body records.
/// Body verbatim from the pre-RFC-20260802 `while` loop; extracted so
/// the any-widen pass can re-drain the worklist after its own clones
/// seed new records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_one_generic(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    generic_fn_names: &mut HashSet<String>,
    item: WorkItem,
) {
    {
        let (name, arg_anns, type_args) = item;
        let Some((type_params, params, return_type, body)) = generics.get(&name) else {
            return;
        };
        let mono_name = cache[&(name.clone(), arg_anns.clone())].clone();
        // RFC 20260810-indirect-argc-abi H1 — a head-less T-31 body's
        // specialization keeps the hidden-argc ABI: mirror the clone
        // into the side table the sig/def/terminal sites key on
        // (`mono_ast` is exactly the AST the lowerer reads). Without
        // this the clone's rewritten `__torajs_argc` reads have no
        // param to bind (loud unknown-ident at lower).
        if owned_ast.headless_argc_fns.contains(&name) {
            owned_ast.headless_argc_fns.insert(mono_name.clone());
        }
        // `MakeIterable$$_boolean` → `$$_boolean`; the same suffix
        // names this spec's lifted-closure clones.
        let spec_suffix = mono_name[name.len()..].to_string();
        // Ann-level substitution — the concrete signature the lowerer
        // parses (keeps width/closure-shape anns like "f64"/"__cls(").
        let subst: Vec<(String, String)> = type_params
            .iter()
            .cloned()
            .zip(arg_anns.iter().cloned())
            .collect();
        let (new_params, new_return_type, new_body, id_map) =
            clone_spec_body(owned_ast, params, return_type, body, &subst);
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
            // checker-internal inference clone -- no own source text
            // (specialized-fn toString via the generic decl's span is
            // an RFC 20260719 B3 refinement)
            span: crate::lexer::Span { start: 0, end: 0 },
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
        // RFC 20260731-mono-closure-clone — clone every lifted closure
        // this spec body references BEFORE the body checks: the clone
        // decls join `owned_ast.stmts` and the spec body's closure
        // sites are rewritten to the clone names, so the spec check's
        // construction-site `check_closure` walks the CLONE bodies
        // (fresh ExprIds) — that walk is what records their inner
        // generic calls for the seed pass below.
        let closure_rename = crate::check_monomorph_closures::clone_spec_closures(
            c,
            owned_ast,
            &spec_suffix,
            &id_map,
            &subst,
        );
        c.check_stmt(owned_ast, &spec_decl);
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
        // The shared originals retire from lowering (their only
        // construction sites lived in generic bodies). The clone
        // decls already joined `owned_ast.stmts` — AFTER this spec
        // decl in stmt order, so construction sites lower before the
        // closure bodies that read their `closure_captures` entries
        // (mono_decls append behind everything).
        generic_fn_names.extend(closure_rename.keys().cloned());
        mono_decls.push(spec_decl);
        // vtable completeness — a `__new_<C>` instantiation queues
        // the class's chain-method specializations under the same
        // suffix (sibling module doc has the full rationale).
        crate::check_monomorph_vtable_seed::seed_class_chain_methods(
            owned_ast,
            generics,
            cache,
            worklist,
            &name,
            type_params.len(),
            &spec_suffix,
            &arg_anns,
            &type_args,
        );
    }
}

/// RFC 20260730-undeclared-ident 刀 3 — prune nowhere-resolving
/// capture names (recorded by check_closure per construction site)
/// from the owned AST's capture lists, so the lowerer's env
/// materialization never sees them; the body's marked Ident read
/// raises the ReferenceError instead. The lifted FnDecl's
/// `__env(...)` ann is kept in step (the lowerer only gates on
/// empty/non-empty, but a stale name list misleads readers). Runs
/// after the mono worklist so specialization-clone closures (fresh
/// ExprIds marked during their body checks) are covered too.
fn prune_unresolved_captures(c: &mut Checker, owned_ast: &mut Ast) {
    for (ceid, gone) in std::mem::take(&mut c.unresolved_captures) {
        let Expr::Closure { fn_name, captures } = &mut owned_ast.exprs[ceid.0 as usize] else {
            continue;
        };
        captures.retain(|cap| !gone.contains(cap));
        let caps = captures.join("|");
        let fname = fn_name.clone();
        for s in owned_ast.stmts.iter_mut() {
            let Stmt::FnDecl { name, params, .. } = s else {
                continue;
            };
            if *name != fname {
                continue;
            }
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({caps})"));
            }
        }
    }
}

/// One specialization's body materialization: ann-substituted params
/// and return type, deep-cloned + substituted body statements, and the
/// clone's ExprId map (feeds the destructuring-group carry done here,
/// plus the lifted-closure reference scan).
///
/// Blade 3 note — the array-destructuring group registry rides the
/// fresh ExprIds so the specialization's own check picks each group's
/// lane; it can differ per instantiation (the same `const [a, b] = xs`
/// destructures an array in one and a generator in the next).
pub(crate) fn clone_spec_body(
    owned_ast: &mut Ast,
    params: &[Param],
    return_type: &Option<String>,
    body: &[Stmt],
    subst: &[(String, String)],
) -> (Vec<Param>, Option<String>, Vec<Stmt>, Vec<(ExprId, ExprId)>) {
    let mut new_params: Vec<Param> = params.to_vec();
    for p in new_params.iter_mut() {
        if let Some(ann) = &mut p.type_ann {
            *ann = substitute_in_ann(ann, subst);
        }
    }
    let new_return_type = return_type.as_ref().map(|rt| substitute_in_ann(rt, subst));
    let mut id_map: Vec<(ExprId, ExprId)> = Vec::new();
    let mut new_body: Vec<Stmt> = body
        .iter()
        .map(|s| crate::ssa_lower::deep_clone_stmt(owned_ast, &mut id_map, s))
        .collect();
    crate::check_monomorph_clone_tables::carry(owned_ast, &id_map);
    for s in new_body.iter_mut() {
        substitute_in_stmt(s, subst);
    }
    for s in new_body.iter() {
        crate::ssa_lower_generics_tvdefault::rewrite_tvdefault_in_stmt(owned_ast, s, subst);
    }
    (new_params, new_return_type, new_body, id_map)
}

/// Resolve each recorded generic call to its mono name: derive the
/// width/closure-shape-aware ann key, reserve the mono name in the
/// shared cache (queueing the instantiation on first sight — the
/// early reservation breaks recursion cycles), and retarget the
/// call's ExprId.
pub(crate) fn seed_records(
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
pub(crate) fn spec_params_env(spec_decl: &Stmt) -> HashMap<String, String> {
    let Stmt::FnDecl { params, .. } = spec_decl else {
        return HashMap::new();
    };
    params
        .iter()
        .filter_map(|p| p.type_ann.as_ref().map(|a| (p.name.clone(), a.clone())))
        .collect()
}
