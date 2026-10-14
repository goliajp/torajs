//! Struct-field / ObjectLit closure-forwarder pass — chunk 354,
//! extracted from ast.rs.
//!
//! Two pub entry points:
//! - `tag_struct_field_closure_types` — retags TypeDecl/ClassDecl
//!   field types from `__fn(P)->(R)` to `__cls(P)->(R)` (Closure ABI
//!   opt-in for struct-field slots only).
//! - `synthesize_fn_to_closure_forwarders` — synths
//!   `__forward_<name>` shims for the ObjectLit-field store-site
//!   `const o: T = { k: top_fn }` where `T.k` was Closure-tagged.
//!
//! Sibling `ast/forwarders.rs` holds the Return-site variant
//! `synthesize_forwarders`.

use super::{Ast, Expr, ExprId, GenericStructDecls, Param, PropKey, Stmt, is_fn_like_ann};
// Decl synthesis lives in the sibling (402-01 file split); the
// re-export keeps both callers' `forwarders_object::` paths intact.
pub(super) use super::forwarders_object_synth::synthesize_forwarder_decls;

/// P3.closure-in-struct-field — rewrites TypeDecl / ClassDecl field
/// types from `__fn(P)->(R)` (parser's internal form of `(P)=>R`) to
/// `__cls(P)->(R)`, so the SSA-layer `parse_type` maps the latter to
/// `Type::Closure` (env-first CallIndirect ABI) while leaving
/// `__fn(P)->(R)` in param / return / let-binding annotations as
/// `Type::FnSig` (direct dispatch, no env overhead).
///
/// This is the narrowest possible Closure-ABI surface that still
/// supports closures-stored-in-struct-fields. Inline struct field
/// slots have to be Closure-typed because users can assign capturing
/// function expressions there (`{ tick: function() { use outer_var
/// } }`); the matching `synthesize_fn_to_closure_forwarders`
/// ObjectLit arm wraps any FnSig store-site (`{ k: top_fn }`) in a
/// trivial forwarder so both shapes reach the slot uniformly.
///
/// Must run AFTER parser type-ann normalization (parser produces
/// `__fn(...)->(R)`) and BEFORE `synthesize_fn_to_closure_forwarders`
/// (which reads tagged field types to know which ObjectLit field
/// position to rewrite). Per-pipeline ordering: after
/// `desugar_classes` (so flattened class field types are visible)
/// and before `lift_arrow_fns` / `synthesize_forwarders` /
/// `synthesize_fn_to_closure_forwarders`.
pub fn tag_struct_field_closure_types(ast: &mut Ast) {
    // RFC 20260710 C5 — an OPTIONAL fn field (`cb?: (n) => R` →
    // `__nullable(__fn(...))`) is the same mutable Closure-repr slot,
    // so `retag_field_fn_ann` handles both spellings.
    fn retag(ann: &mut String) {
        *ann = super::lift_arrow_fns::retag_field_fn_ann(ann);
    }
    for s in &mut ast.stmts {
        match s {
            Stmt::TypeDecl { fields, .. } => {
                for (_, fty) in fields {
                    retag(fty);
                }
            }
            Stmt::ClassDecl { fields, .. } => {
                for (_, fty) in fields {
                    retag(fty);
                }
            }
            _ => {}
        }
    }
}

/// P3.closure-in-struct-field — narrows the Closure-typed slot
/// surface: only inline-struct field types tagged by
/// `tag_struct_field_closure_types` (annotation rewritten from
/// `(P)=>R` to `__cls(P)->(R)`) end up as Type::Closure at the SSA
/// layer. Fn-typed params / returns / let bindings stay as
/// Type::FnSig (direct call ABI; no env-first overhead).
///
/// In the one remaining store-site that needs wrapping — `const o: T
/// = { k: top_fn }` where `T.k` was tagged Closure — this pass
/// synthesizes a trivial `__forward_<top_fn>(__env, args...) { return
/// top_fn(args...); }` closure-shaped FnDecl and rewrites the bare
/// `top_fn` Ident in the ObjectLit field to a
/// `Closure { fn_name: "__forward_<top_fn>", captures: [] }` value.
/// Lifted function expressions (`lift_arrow_fns` output) already
/// arrive as Closure values, so they don't need rewriting here.
///
/// Strategy mirrors `synthesize_forwarders`: per unique target name
/// synth one `__forward_<name>(__env, args...) { return name(args...); }`
/// closure-shaped FnDecl, then rewrite each store-site's Ident to
/// `Closure { fn_name: "__forward_<name>", captures: [] }`. Idempotent
/// across multiple store-sites for the same target.
///
/// Runs after `synthesize_forwarders` so Return-site renames already
/// happened; we extend coverage to the remaining three store-sites.
/// Snapshot non-closure-shaped FnDecls' signatures (for forwarder
/// body synthesis) plus the already-synthesized forwarder names and
/// each generic fn's type-param list (the generic-param call-arg
/// axis: a fn-name argument to `sameValue<T>(actual: T, expected:
/// T)` instantiates T at Any, which can't box a raw FnSig). Skips
/// forwarders themselves (`__forward_*`) and closure-shaped fns
/// (first param `__env`).
#[allow(clippy::type_complexity)]
pub(super) fn snapshot_fn_sigs(
    ast: &Ast,
) -> (
    std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    std::collections::HashSet<String>,
    std::collections::HashMap<String, Vec<String>>,
) {
    let mut fn_sigs = std::collections::HashMap::new();
    let mut existing_forwarders = std::collections::HashSet::new();
    let mut fn_type_params = std::collections::HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            type_params,
            span,
            ..
        } = s
        {
            if super::is_shell(name) {
                existing_forwarders.insert(name.clone());
                continue;
            }
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if !is_closure_shaped {
                fn_sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
                if !type_params.is_empty() {
                    fn_type_params.insert(name.clone(), type_params.clone());
                }
            }
        }
    }
    (fn_sigs, existing_forwarders, fn_type_params)
}

/// The TypeDecl snapshots the ObjectLit-field store-site check
/// resolves `const o: T = { k: name }` against: (struct_name →
/// field_name → field_ann) plus the chunk-795 generic TypeDecl
/// halves for `Box<() => number>` instantiation resolution.
#[allow(clippy::type_complexity)]
fn snapshot_type_field_anns(
    ast: &Ast,
) -> (
    std::collections::HashMap<String, std::collections::HashMap<PropKey, String>>,
    GenericStructDecls,
) {
    let mut struct_field_anns = std::collections::HashMap::new();
    let mut generic_field_anns = std::collections::HashMap::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        {
            let map: std::collections::HashMap<PropKey, String> =
                fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            struct_field_anns.insert(name.clone(), map);
            if !type_params.is_empty() {
                generic_field_anns.insert(
                    name.clone(),
                    ast.generic_struct_decl(name, type_params, fields),
                );
            }
        }
    }
    (struct_field_anns, generic_field_anns)
}

pub fn synthesize_fn_to_closure_forwarders(ast: &mut Ast) {
    use std::collections::HashSet;

    let (mut fn_sigs, existing_forwarders, fn_type_params) = snapshot_fn_sigs(ast);
    let block_scoped = super::namedfn_rebound_scope::refuse_rebound_names(ast, &mut fn_sigs);
    // 424-04 — census the fn-typed bindings whose store faces
    // mismatch their annotation BEFORE the rewrites below turn those
    // store rhs Idents into Expr::Closure (the census keys off the
    // bare Ident). The lowering routes the marked bindings through
    // the boxed dual entry (module doc in the sibling). It runs
    // whether or not the module declares a named function: a store
    // of a function literal needs none.
    let mismatch =
        super::forwarders_object_mismatch::collect_fnsig_mismatch_bindings(ast, &fn_sigs);
    ast.fnsig_mismatch_bindings.extend(mismatch);
    for (f, ps) in super::forwarders_object_mismatch::collect_fn_param_mismatches(ast) {
        ast.argv_boxed_params.entry(f).or_default().extend(ps);
    }
    if fn_sigs.is_empty() {
        return;
    }

    let (struct_field_anns, generic_field_anns) = snapshot_type_field_anns(ast);

    // RFC 20260806 — which builtin methods this module might patch.
    // A patched one stands down to the any-lane, so a fn-name
    // argument at such a call site has to arrive as a closure cell.
    // Scanning here rather than reusing the checker's copy keeps this
    // pass pre-typecheck: the scan is pure syntax and wants no types.
    let proto_shadow = crate::builtin_proto_shadow::collect_shadowed_builtin_methods(ast);

    // Binding names declared `any` anywhere — the assign-into-any
    // axis matches `f = top_fn` against these (scope-approximate).
    let mut any_bindings: HashSet<String> = HashSet::new();
    crate::ast_collect_fn_closure::collect_any_bindings(&ast.stmts, &mut any_bindings);

    // Chunk 733 — binding names declared with a fn-typed array ann;
    // their element slots are Closure-repr, so `fns.push(top_fn)` /
    // `fns[i] = top_fn` store-sites wrap.
    let mut fn_arr_bindings: HashSet<String> = HashSet::new();
    crate::ast_collect_fn_closure::collect_fn_arr_bindings(&ast.stmts, &mut fn_arr_bindings);

    // RFC 20260709-closure-global chunk 4 — top-level Closure-repr
    // binding names (lifted-arrow init or fn-type annotation): the
    // assign axis wraps `cb = top_fn` so the global assign lane
    // stores a closure cell. Chunk 805 — a bare named-fn Ident init
    // that the let-init axis below will wrap joins too (mirror of
    // that axis's gate): the binding becomes a Closure-repr global,
    // so a body-local `op = other_fn` re-assign must wrap as well or
    // the assign lane stores a raw FnSig into the Closure slot.
    let binding_refs = crate::ast_refs::toplevel_binding_refs(ast);
    let mut closure_bindings: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
            is_var: false,
            ..
        } = s
            && (matches!(ast.get_expr(*init), Expr::Closure { .. })
                || type_ann.as_deref().is_some_and(is_fn_like_ann)
                || (type_ann.is_none()
                    && binding_refs.named_fn_refs.contains(name)
                    && matches!(ast.get_expr(*init), Expr::Ident(n) if fn_sigs.contains_key(n))))
        {
            closure_bindings.insert(name.clone());
        }
    }
    // Chunk 736 — fn-body fn-type-annotated bindings join the set
    // recursively (the top-level walk above missed them, so a
    // body-local `cb = top_fn` assign escaped the wrap after the
    // mutable-init axis moved such slots to Closure repr).
    crate::ast_collect_fn_closure::collect_fn_ann_bindings(&ast.stmts, &mut closure_bindings);

    // Walk all top-level stmts (including FnDecl bodies recursively)
    // collecting the store-sites where a bare top-FnDecl Ident needs
    // the forwarder wrap (see ast_collect_fn_closure module doc for
    // the axis list).
    let stmts_snapshot = ast.stmts.clone();
    // V1b — un-annotated ctor-init receivers (see the collect's doc).
    let mut new_init_bindings: HashSet<String> = HashSet::new();
    crate::ast_collect_bindings::collect_untyped_new_init_bindings(
        ast,
        &ast.stmts,
        &mut new_init_bindings,
    );
    let ident_census = crate::ast_collect_fn_closure_init::ident_census(ast);
    let mut mixed_slot_rows = crate::ast_collect_fn_closure_target::mixed_slot_rows(ast, &fn_sigs);
    // a function handed to a parameter returning a bare fn type joins
    // the lane with that parameter's other functions, mixed or not
    mixed_slot_rows
        .extend(crate::ast_collect_fn_closure_target::raw_ret::fn_ret_param_args(ast, &fn_sigs));
    let param_arg_targets = crate::ast_collect_fn_closure_target::raw_ret::param_arg_targets(
        ast,
        &fn_sigs,
        &mixed_slot_rows,
    );
    let mut collector = crate::ast_collect_fn_closure::FnToClosureCollector {
        ast,
        fn_sigs: &fn_sigs,
        fn_type_params: &fn_type_params,
        struct_field_anns: &struct_field_anns,
        generic_field_anns: &generic_field_anns,
        any_bindings: &any_bindings,
        new_init_bindings: &new_init_bindings,
        closure_bindings: &closure_bindings,
        fn_arr_bindings: &fn_arr_bindings,
        proto_shadow: &proto_shadow,
        ident_census: &ident_census,
        mixed_slot_rows: &mixed_slot_rows,
        targets: HashSet::new(),
        rewrites: Vec::new(),
        shadowed: Vec::new(),
        any_param_frames: Vec::new(),
        cls_param_frames: Vec::new(),
    };
    for s in &stmts_snapshot {
        collector.walk_stmt(s, false);
    }
    collector.targets.extend(param_arg_targets);
    // RFC 20260729-fn-value-any V4 刀 1 — a destructuring-slot
    // default that is a hoisted generator-expression factory
    // (`{ gen = function* () {} }` → the ternary's else arm holds
    // `Ident("__genexpr_N")`): the slot is an any destination the
    // walk has no axis for, and the raw FnSig panics at box_to_any
    // (the t262 fn-name-gen template family, 186 of the 291
    // residue). The synthetic prefix is the guard — user code
    // cannot spell it, so no shadow risk — and wrapping a gen
    // factory here accepts the same intrinsic-chain trade the
    // any-call-arg axis has always made. `.name` still answers the
    // synthetic name (loud assert mismatch, not a whole-program
    // reject); the NamedEvaluation thread through
    // `dstr_default_names` is the registered follow-up.
    for &eid in ast.dstr_default_names.clone().keys() {
        if matches!(ast.get_expr(eid), Expr::Ident(n) if n.starts_with("__genexpr_")) {
            collector.try_mark(eid);
        }
    }
    // The member-assign twin of the same shape, and the same
    // any-destination the walk has no axis for: `this.k =
    // function* () {}` leaves `Ident("__genexpr_N")` as the value of
    // an assignment into an any-typed field. The raw FnSig stored
    // there UNBOXED (`store <fn_addr>, %__this +32`), and the field's
    // drop then rc_dec'd a code address — SIGBUS at program exit,
    // with the program's own output already printed. A plain
    // function expression never got there: the collector's own axes
    // turn it into a closure cell first.
    let genexpr_assign_values: Vec<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Assign { target, value }
                if matches!(
                    ast.get_expr(*target),
                    Expr::Member { .. } | Expr::Index { .. }
                ) && matches!(ast.get_expr(*value), Expr::Ident(n) if n.starts_with("__genexpr_")) =>
            {
                Some(*value)
            }
            _ => None,
        })
        .collect();
    for eid in genexpr_assign_values {
        collector.try_mark(eid);
    }
    let (mut targets, mut rewrites) = (collector.targets, collector.rewrites);

    let mut variadic_bindings: Vec<String> = Vec::new();
    super::forwarders_let_init_axis::collect_let_init_axis_rewrites(
        ast,
        &stmts_snapshot,
        &binding_refs,
        &fn_sigs,
        &mut targets,
        &mut rewrites,
        &mut variadic_bindings,
    );
    ast.variadic_value_bindings.extend(variadic_bindings);
    super::variadic_closure_bindings::record(ast);

    rewrites.retain(|(eid, _)| !block_scoped.contains(eid));
    if rewrites.is_empty() && targets.is_empty() {
        return;
    }

    let (new_decls, renames) =
        synthesize_forwarder_decls(ast, &targets, &fn_sigs, &existing_forwarders);

    // Apply rewrites.
    for (eid, target) in rewrites {
        if let Some(forward_name) = renames.get(&target) {
            ast.exprs[eid.0 as usize] = Expr::Closure {
                fn_name: forward_name.clone(),
                captures: Vec::new(),
            };
        }
    }

    ast.stmts.extend(new_decls);
}
