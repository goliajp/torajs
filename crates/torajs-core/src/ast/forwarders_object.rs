//! Struct-field / ObjectLit closure-forwarder pass — chunk 354,
//! extracted from ast.rs.
//!
//! Two pub entry points:
//! - `tag_struct_field_closure_types` — retags TypeDecl/ClassDecl
//!   field types from `__fn(P)->R` to `__cls(P)->R` (Closure ABI
//!   opt-in for struct-field slots only).
//! - `synthesize_fn_to_closure_forwarders` — synths
//!   `__forward_<name>` shims for the ObjectLit-field store-site
//!   `const o: T = { k: top_fn }` where `T.k` was Closure-tagged.
//!
//! Sibling `ast/forwarders.rs` holds the Return-site variant
//! `synthesize_forwarders`.

use super::{Ast, Expr, ExprId, Param, Stmt, is_fn_like_ann};

/// P3.closure-in-struct-field — rewrites TypeDecl / ClassDecl field
/// types from `__fn(P)->R` (parser's internal form of `(P)=>R`) to
/// `__cls(P)->R`, so the SSA-layer `parse_type` maps the latter to
/// `Type::Closure` (env-first CallIndirect ABI) while leaving
/// `__fn(P)->R` in param / return / let-binding annotations as
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
/// `__fn(...)->R`) and BEFORE `synthesize_fn_to_closure_forwarders`
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
/// `(P)=>R` to `__cls(P)->R`) end up as Type::Closure at the SSA
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
pub fn synthesize_fn_to_closure_forwarders(ast: &mut Ast) {
    use std::collections::{HashMap, HashSet};

    // Snapshot non-closure-shaped FnDecls' signatures (for forwarder
    // body synthesis). Skip forwarders themselves (`__forward_*`) and
    // closure-shaped fns (first param `__env`).
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> =
        HashMap::new();
    let mut existing_forwarders: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        {
            if name.starts_with("__forward_") {
                existing_forwarders.insert(name.clone());
                continue;
            }
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if !is_closure_shaped {
                fn_sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
            }
        }
    }
    if fn_sigs.is_empty() {
        return;
    }

    // Collect (struct_name, field_name → field_ann) for type-aliased
    // struct shapes — used by the ObjectLit-field store-site check to
    // resolve `const o: T = { k: name }` against `T`'s declared field
    // types.
    let mut struct_field_anns: HashMap<String, HashMap<String, String>> = HashMap::new();
    // Chunk 795 — generic TypeDecl snapshots for the wrap axes'
    // `Box<() => number>` instantiation resolution.
    let mut generic_field_anns: HashMap<String, (Vec<String>, Vec<(String, String)>)> =
        HashMap::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        {
            let map: HashMap<String, String> =
                fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            struct_field_anns.insert(name.clone(), map);
            if !type_params.is_empty() {
                generic_field_anns.insert(name.clone(), (type_params.clone(), fields.clone()));
            }
        }
    }

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

    // Chunk 783 — binding name → declared struct annotation, for
    // the member-assign axis (`o.cb = top_fn` where the receiver's
    // field is fn-typed). Chunk 793 — inline object types
    // (`__inlobj(...)`) join named TypeDecl names; the collector's
    // `resolve_field_anns` decodes both.
    let mut struct_bindings: HashMap<String, String> = HashMap::new();
    let is_generic_inst = |a: &str| {
        let t = a.trim();
        t.find('<')
            .is_some_and(|i| t.ends_with('>') && generic_field_anns.contains_key(&t[..i]))
    };
    crate::ast_collect_bindings::collect_bindings_ann_matching(
        &ast.stmts,
        &|a| {
            struct_field_anns.contains_key(a.trim())
                || crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(a).is_some()
                || is_generic_inst(a)
        },
        &mut struct_bindings,
    );
    // Chunk 790 — binding name → struct-ARRAY annotation, for the
    // element-receiver face of the same axis (`arr[0].cb = top_fn`).
    let mut struct_arr_bindings: HashMap<String, String> = HashMap::new();
    crate::ast_collect_bindings::collect_bindings_ann_matching(
        &ast.stmts,
        &|a| {
            crate::ast_collect_fn_closure::strip_arr_ann(a).is_some_and(|e| {
                struct_field_anns.contains_key(e)
                    || crate::ast_collect_fn_closure_init::parse_inlobj_field_anns(e).is_some()
                    || is_generic_inst(e)
            })
        },
        &mut struct_arr_bindings,
    );

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
    let mut collector = crate::ast_collect_fn_closure::FnToClosureCollector {
        ast,
        fn_sigs: &fn_sigs,
        struct_field_anns: &struct_field_anns,
        generic_field_anns: &generic_field_anns,
        any_bindings: &any_bindings,
        new_init_bindings: &new_init_bindings,
        closure_bindings: &closure_bindings,
        fn_arr_bindings: &fn_arr_bindings,
        struct_bindings: &struct_bindings,
        struct_arr_bindings: &struct_arr_bindings,
        targets: HashSet::new(),
        rewrites: Vec::new(),
        shadowed: Vec::new(),
    };
    for s in &stmts_snapshot {
        collector.walk_stmt(s, false);
    }
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
    let (mut targets, mut rewrites) = (collector.targets, collector.rewrites);

    collect_let_init_axis_rewrites(
        ast,
        &stmts_snapshot,
        &binding_refs,
        &fn_sigs,
        &mut targets,
        &mut rewrites,
    );

    if rewrites.is_empty() {
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

/// RFC 20260709-closure-global chunk 4 — the let-init axis:
/// `const f: (xs)=>n = top_fn` (or un-annotated) at the TOP level
/// where a named-fn body reads `f`. The wrap turns the init into a
/// lifted-arrow shape so the K.3b promote fires and the named fn
/// reaches the binding through the global-closure call lane. Gated
/// on the same ast_refs pair the promote uses — a main-only
/// binding keeps the direct-dispatch fn_addr_let home, and a
/// closure-captured one stays main-local (a slot would split the
/// binding into two homes).
fn collect_let_init_axis_rewrites(
    ast: &super::Ast,
    stmts_snapshot: &[Stmt],
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    for s in stmts_snapshot {
        if let Stmt::LetDecl {
            name,
            init,
            type_ann,
            is_var: false,
            ..
        } = s
            && type_ann
                .as_deref()
                .is_none_or(|a| is_fn_like_ann(a) && !a.contains("__rest("))
            && binding_refs.named_fn_refs.contains(name)
            // Chunk 737 — immutable closure-captured bindings promote
            // (the capture filter resolves them to the global), so
            // their named-fn inits wrap too. Chunk 740 — the
            // mutable+captured combination promotes the same way
            // (capture filter reads + Assign-Ident global writes =
            // one home), so its named-fn init wraps too.
            && let Expr::Ident(n) = ast.get_expr(*init)
            // Chunk 805 — no explicit-return-ann gate anymore: the
            // forwarder clones the target's `None` ret, and
            // `desugar_implicit_generics` (which runs AFTER this
            // pass) backfills it — its `__env` arm infers the
            // forwarder's `return target(...)` from the target's own
            // backfilled ret. A target with no value returns stays
            // `None` and the synthesized canon spells `void`, which
            // is exactly right for it. The other wrap axes (any /
            // assign / struct-field) have shipped this un-annotated
            // path all along.
            && fn_sigs.contains_key(n)
        {
            targets.insert(n.clone());
            rewrites.push((*init, n.clone()));
        }
    }
}

/// Synthesize one `__forward_<target>(__env, args...) { return
/// target(args...); }` closure-shaped FnDecl per unique target
/// (skipping any `synthesize_forwarders` already produced), returning
/// the decls plus the target → forwarder-name rename map (chunk 783
/// extraction — the member-assign axis pushed the caller past the
/// 200-line fn limit).
fn synthesize_forwarder_decls(
    ast: &mut Ast,
    targets: &std::collections::HashSet<String>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    existing_forwarders: &std::collections::HashSet<String>,
) -> (Vec<Stmt>, std::collections::HashMap<String, String>) {
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for target in targets {
        let forward_name = format!("__forward_{target}");
        if existing_forwarders.contains(&forward_name) {
            renames.insert(target.clone(), forward_name);
            continue;
        }
        let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
        let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
        fwd_params.push(Param {
            name: "__env".into(),
            type_ann: Some("__env()".to_string()),
            default: None,
            is_rest: false,
        });
        // Skip a promoted target's hidden `__this` first param on the
        // public face; forward `undefined` into it (mirrors
        // forwarders.rs — plain-call receiver semantics).
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        let user_params = if takes_this {
            &params[1..]
        } else {
            &params[..]
        };
        // Knife 4d — a trailing GEN_ARGV_PARAM never enters the
        // forwarder's declared face; the call below feeds it
        // `[...arguments]` (see forwarders::split_gen_argv_tail).
        let (user_params, takes_gen_argv) = super::forwarders::split_gen_argv_tail(user_params);
        fwd_params.extend(user_params.iter().cloned());
        let mut arg_eids: Vec<ExprId> = Vec::with_capacity(params.len());
        if takes_this {
            arg_eids.push(ast.add_expr(Expr::Ident("undefined".into())));
        }
        for p in user_params {
            arg_eids.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        if takes_gen_argv {
            super::forwarders::push_gen_argv_spread(ast, &mut arg_eids);
        }
        let callee_id = ast.add_expr(Expr::Ident(target.clone()));
        let call_id = ast.add_expr(Expr::Call {
            callee: callee_id,
            args: arg_eids,
        });
        let body = vec![Stmt::Return(Some(call_id))];
        new_decls.push(Stmt::FnDecl {
            name: forward_name.clone(),
            type_params: Vec::new(),
            params: fwd_params,
            return_type,
            body,
            is_generator: false,
            // B4 — carry the TARGET's source span (toString answers
            // the wrapped user fn's source).
            span: target_span,
        });
        renames.insert(target.clone(), forward_name);
    }
    (new_decls, renames)
}
