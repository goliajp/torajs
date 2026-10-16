//! `desugar_implicit_generics` extracted from [`crate::ast`]
//! (chunk 140).
//!
//! Pre-extract this was a 294 LOC `pub fn` inline in ast.rs (over
//! the 200-line god-fn hard limit per `torajs-file-size-debt`).
//! Body verbatim moved here; ast.rs keeps a 1-line wrapper.
//!
//! Three independent transforms over top-level FnDecls:
//!
//! 1. **Closure-shaped FnDecls** (`__env` / `__this` first param)
//!    get unannotated remaining params defaulted to `Type::Any`
//!    (capturing arrows + class methods both need this so call-site
//!    boxing works). A return-ann sniff runs when `return_type` is
//!    None and the body has at least one value return.
//! 2. **Lifted closure FnDecls** (`__closure_*`) — same Any default
//!    for non-`__env` / non-`__this` params, then return-ann sniff.
//!    Defaulting to Any sidesteps the indirect-call-retargeter's
//!    bare-Ident-only limitation; the signature is concrete from
//!    the start so no monomorphization is needed.
//! 3. **Plain user FnDecls** — unannotated non-rest params get
//!    fresh `__T<N>` TypeVars allocated (skipping any name already
//!    in `type_params`). Rest params (`...args: any[]`) are left
//!    un-genericized (no list-of-T encoding yet). Explicit `: any`
//!    stays literal "any" (P0.9). Return type:
//!    - `: any` → stays literal (P0.9 — Any-aware BinOp / return-
//!      assignability already handle Any-on-LHS).
//!    - omitted → return-ann sniff (`__T1` if body returns the
//!      param verbatim; concrete type if all returns agree;
//!      None otherwise → typecheck rejects with the existing
//!      "requires annotation" error).
//!    - explicit non-any → leave alone.
//!
//! Helper inputs (all `pub(crate)` in ast.rs):
//! `binds_to_params`, `infer_expr_ann_with`, `body_has_value_return`,
//! `infer_return_ann`, `AstExprsView`.

mod declared_sigs;

use std::collections::HashSet;

use crate::ast::{
    Ast, AstExprsView, Param, Stmt, binds_to_params, body_has_value_return, collect_outer_binds,
    infer_expr_ann_with, infer_return_ann,
};
use crate::ast_desugar_implicit_generics_closure::{
    desugar_closure_shape_fn, desugar_lifted_closure_fn, preinfer_closure_sigs, receiver_field_rows,
};

pub(crate) fn run(ast: &mut Ast) {
    // A class factory reached through a VALUE gets its arguments as
    // `any` off argv, and the ctor registry keys the adapter on the
    // bare `__new_<C>` spelling — a GENERIC factory has no instance
    // under that name when no static call site drives one (`const
    // nd: any = C; new nd(7)` died on "cannot be reached through a
    // runtime value"). When the program constructs from values, keep
    // factories non-generic: untyped ctor params annotate `any`
    // instead (rotation 409; the demand predicate is the same one
    // that arms the adapter synthesis).
    let factories_stay_any = crate::ssa_lower_boxed_entry::program_constructs_from_value(ast);
    let always_any_factories = always_any_factories(ast);
    // (k) leg — needs the whole `&Ast` (named-fn refs + the shared
    // promote verdict), so it runs before the borrow split below.
    let promoted_objlits = crate::ast_refs_any_promote::objlit::promoted_method_objlits(ast);
    // Read before the borrow split: it is two fields, and the split
    // hands out only one of them.
    let sloppy = ast.program_is_sloppy();
    let type_seeds =
        crate::ast::implicit_generics_infer_call::object_type_seeds::object_type_seeds(ast);
    let Ast {
        stmts,
        exprs,
        expr_spans,
        objlit_method_exprs,
        objlit_method_fields,
        objlit_shorthand_proto_exprs,
        fn_expr_exprs,
        closure_argc_locals,
        closure_argv_fns,
        closure_argv_locals,
        fnexpr_recv_fns,
        fnexpr_recv_faces,
        fnexpr_recv_locals,
        fnexpr_recv_foreign_uses,
        fnsig_mismatch_bindings,
        escaped_use_strict,
        objlit_computed_keys,
        objlit_computed_accessors,
        ..
    } = ast;
    let sloppy = sloppy.then_some(&*escaped_use_strict);
    let ast_exprs_view: AstExprsView = &*exprs;

    let mut fn_sigs = declared_sigs::seed_declared_fn_sigs(stmts);

    let mut outer_binds: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    collect_outer_binds(stmts, ast_exprs_view, &fn_sigs, &mut outer_binds);

    // Construction-site annotation snapshots (closure_capture_anns) —
    // the return-ann sniff resolves a closure's captured idents (and
    // objlit_nominal a method-carrying literal's data-field idents)
    // against the scope of the construction site, falling back to the
    // program-wide by-name `outer_binds` only on snapshot misses. Two
    // same-named params in unrelated fns used to race on last-seen
    // insertion order and flip the sniffed ann (`any` / `string`),
    // drifting the closure ABI / `__ObjLit_n` layout away from the
    // real value repr (tasks/2026-07-18 num-width Captured-broadcast
    // poison).
    let site_anns: crate::ast::SiteAnns = crate::ast::collect_closure_capture_anns(
        stmts,
        ast_exprs_view,
        &fn_sigs,
        objlit_method_exprs,
        fn_expr_exprs,
    );
    let cap_anns = &site_anns.closures;

    // RFC 20260704 C4+ (chunk 522) — pre-infer the lifted-closure
    // signatures so a closure-typed local can bubble out of a fn as
    // its return type. The main loop below visits stmts in source
    // order and the lifted `__closure_*` decls sit at the tail, so a
    // user fn returning a closure local (`const g = (x) => ...;
    // return g`) was inferred BEFORE the closure's own return type
    // existed — the Ident lookup missed and the fn stayed Void.
    // Running the closure branches first (identical, idempotent
    // logic) + publishing each closure's full `__fn(P|..)->(R)` ann
    // under its reserved `__closure_*` name lets
    // `infer_expr_ann_with`'s Expr::Closure arm answer fn-shaped
    // anns; `parse_type` maps `__fn` to FnSig and `effective_ret_ty`
    // upgrades to Closure where the body returns closure values.
    preinfer_closure_sigs(
        stmts,
        ast_exprs_view,
        &outer_binds,
        cap_anns,
        closure_argv_fns,
        &type_seeds,
        &mut fn_sigs,
    );
    // RFC 20260714-objlit-accessor blade 1 — must sit between the
    // pre-infer above and the main loop below: the lifted closures and
    // `fn_sigs` exist by now (so a method's FnDecl can take `__this` and
    // re-publish its sig as `__mth(`), while the object literal's own
    // `__inlobj(...)` ann is minted by the loop below — which is what
    // carries the `__mth(` field out to the checker and the lowerer.
    crate::ast::objlit_nominal::run(
        stmts,
        exprs,
        sloppy,
        expr_spans,
        objlit_method_exprs,
        objlit_shorthand_proto_exprs,
        objlit_method_fields,
        &outer_binds,
        &site_anns.objlits,
        &mut fn_sigs,
        fnexpr_recv_fns,
        objlit_computed_keys,
        objlit_computed_accessors,
        &promoted_objlits,
    );
    // RFC 20260717-fnexpr-this-channel knife 1 — same slot rationale as
    // objlit_nominal above: the lifted closures exist and
    // `preinfer_closure_sigs` has published the user-facing (and
    // deliberately `__this`-free) `__fn(` anns, so inserting the
    // receiver param here is invisible to every sig consumer.
    crate::ast::fnexpr_this::run(
        stmts,
        exprs,
        sloppy,
        fn_expr_exprs,
        objlit_method_exprs,
        closure_argc_locals,
        closure_argv_locals,
        fnexpr_recv_fns,
        fnexpr_recv_faces,
        fnexpr_recv_locals,
        fnexpr_recv_foreign_uses,
        fnsig_mismatch_bindings,
        expr_spans,
    );
    let ast_exprs_view: AstExprsView = &*exprs;
    backfill_closure_typed_lets(stmts, ast_exprs_view, &mut outer_binds, &fn_sigs);

    let receiver_fields = receiver_field_rows(stmts);

    for stmt in stmts.iter_mut() {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            type_params,
            body,
            ..
        } = stmt
        else {
            continue;
        };

        let first_kind = params.first().map(|p| p.name.clone());
        if matches!(first_kind.as_deref(), Some("__env") | Some("__this")) {
            desugar_closure_shape_fn(
                first_kind.as_deref(),
                name,
                params,
                return_type,
                body,
                ast_exprs_view,
                &outer_binds,
                cap_anns,
                &receiver_fields,
                &type_seeds,
                &mut fn_sigs,
            );
            continue;
        }
        if name.starts_with("__closure_") {
            desugar_lifted_closure_fn(params, return_type, body, ast_exprs_view, &fn_sigs);
            continue;
        }
        if (factories_stay_any && name.starts_with("__new_"))
            || always_any_factories.contains(name.as_str())
        {
            for p in params.iter_mut() {
                if p.type_ann.is_none() && !p.is_rest {
                    p.type_ann = Some("any".to_string());
                }
            }
            continue;
        }

        desugar_user_fn(
            name,
            params,
            return_type,
            type_params,
            body,
            ast_exprs_view,
            &mut fn_sigs,
        );
    }
}

/// …and, whatever the program spells, the factories a runtime
/// consumer constructs through (`factory_always_admitted`).
fn always_any_factories(ast: &Ast) -> std::collections::HashSet<String> {
    let heirs = crate::ssa_lower_boxed_entry::promise_heir_classes(ast);
    ast.stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => name.strip_prefix("__new_"),
            _ => None,
        })
        .filter(|c| crate::ssa_lower_boxed_entry::factory_always_admitted(ast, &heirs, c))
        .map(|c| format!("__new_{c}"))
        .collect()
}

/// Second pass over top-level lets — a `const h = <closure>` binding
/// could not resolve before the closure sigs existed
/// (`preinfer_closure_sigs` publishes them just before this runs).
fn backfill_closure_typed_lets(
    stmts: &[Stmt],
    ast_exprs_view: AstExprsView,
    outer_binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    for s in stmts.iter() {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && !outer_binds.contains_key(name)
        {
            let bs: Vec<Param> = binds_to_params(outer_binds);
            if let Some(ann) = infer_expr_ann_with(ast_exprs_view, *init, &bs, outer_binds, fn_sigs)
            {
                outer_binds.insert(name.clone(), ann);
            }
        }
    }
}

/// Main-loop's "plain user fn" arm — auto-generic every un-annotated
/// param (skipping rest-params), infer / fallback the return type per
/// chunk 613 "un-typeable value return → `any` instead of Void", and
/// republish the resolved `name → return_type` binding into `fn_sigs`
/// so later fns in source order can sniff a `return <this-fn>(...)`
/// chain. Skips auto-generic `__T*` rets — meaningless outside this
/// fn. Runs after the two closure arms in the main loop (`__env` /
/// `__this` / `__closure_` — the closure arms `continue` before
/// reaching here).
fn desugar_user_fn(
    name: &str,
    params: &mut [Param],
    return_type: &mut Option<String>,
    type_params: &mut Vec<String>,
    body: &[Stmt],
    ast_exprs_view: AstExprsView,
    fn_sigs: &mut std::collections::HashMap<String, String>,
) {
    let mut taken: HashSet<String> = type_params.iter().cloned().collect();

    let mut next_idx: usize = type_params.len();
    let alloc = |taken: &mut HashSet<String>, next_idx: &mut usize| -> String {
        loop {
            *next_idx += 1;
            let candidate = format!("__T{next_idx}");
            if !taken.contains(&candidate) {
                taken.insert(candidate.clone());
                return candidate;
            }
        }
    };

    let mut new_type_params: Vec<String> = Vec::new();
    for p in params.iter_mut() {
        let needs_var = p.type_ann.is_none();
        if !needs_var {
            continue;
        }
        if p.is_rest {
            continue;
        }
        let var_name = alloc(&mut taken, &mut next_idx);
        p.type_ann = Some(var_name.clone());
        new_type_params.push(var_name);
    }

    if return_type.as_deref() == Some("any") {
        // P0.9 — explicit `: any` return stays literal "any"; no rewrite.
    } else if return_type.is_none() && body_has_value_return(body) {
        if !crate::ast::body_always_terminates(body) {
            // Value returns AND a reachable fall-through (ES
            // §10.2.1.4 step 11: that path answers `undefined`). A
            // concrete scalar inference (`boolean` off `if (c)
            // return true;`) would leave the lowering's tail close
            // with no undefined spelling — Bool/I64 slots have no
            // sentinel bit pattern, so the open block terminated
            // `unreachable` and running off the end trapped
            // (SIGTRAP, no output; test262 10.6-10-c-ii-1 hit it).
            // `any` carries both the value and the undefined.
            *return_type = Some("any".to_string());
        } else if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params, fn_sigs) {
            *return_type = Some(inferred);
        } else {
            // Chunk 613 — mirror the closure arms: a value return
            // the sniff can't type must not stay None (the checker
            // then expects Void and rejects `return f(...)` chains
            // through untyped fns). Fall back to `any`.
            *return_type = Some("any".to_string());
        }
    }

    // Chunk 613 — publish the inferred ret so LATER fns in source
    // order can sniff a `return <this-fn>(...)` chain (the a2d
    // shape: untyped inner + outer returning inner's call). Skip
    // auto-generic `__T*` rets — meaningless outside this fn.
    if let Some(rt) = return_type.as_ref()
        && !rt.starts_with("__T")
    {
        fn_sigs.insert(name.to_string(), rt.clone());
    }

    if !new_type_params.is_empty() {
        type_params.extend(new_type_params);
    }
}

pub(crate) fn is_synth_closure_name(name: &str) -> bool {
    // The two reserved names a `Expr::Closure` value can carry:
    // `lift_arrow_fns`' lifted arrows and
    // `synthesize_fn_to_closure_forwarders`' `__forward_<fn>` shims
    // (both closure-shaped, `__env`-first). `infer_expr_ann_with`'s
    // `Expr::Closure` arm reads their FULL `__fn(P|..)->(R)` ann out of
    // `fn_sigs`, whereas the same map holds a BARE return ann for user
    // fns — so a name that reaches that arm without being published
    // here silently reads back the return type as if it were the fn
    // type (`{ g: top }` inferred `g: number`, not `g: () => number`).
    // Both namespaces are unspellable in user source, so the shared
    // map stays unambiguous.
    name.starts_with("__closure_")
        || name.starts_with("__forward_")
        // Receiver-first callback forwarders (ast/namedfn_recv_cb) —
        // same closure-shaped synth namespace, same full-ann read.
        || name.starts_with("__fwdrecv_")
}
