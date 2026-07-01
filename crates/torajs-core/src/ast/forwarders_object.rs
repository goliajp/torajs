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

use super::{Ast, Expr, ExprId, Param, Stmt};

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
    fn retag(ann: &mut String) {
        if let Some(rest) = ann.strip_prefix("__fn(") {
            let new_ann = format!("__cls({rest}");
            *ann = new_ann;
        }
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
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>)> = HashMap::new();
    let mut existing_forwarders: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } = s
        {
            if name.starts_with("__forward_") {
                existing_forwarders.insert(name.clone());
                continue;
            }
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if !is_closure_shaped {
                fn_sigs.insert(name.clone(), (params.clone(), return_type.clone()));
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
    for s in &ast.stmts {
        if let Stmt::TypeDecl { name, fields, .. } = s {
            let map: HashMap<String, String> =
                fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            struct_field_anns.insert(name.clone(), map);
        }
    }

    // Collect ExprId → target_name rewrites and the set of targets
    // that need forwarders.
    let mut targets: HashSet<String> = HashSet::new();
    let mut rewrites: Vec<(ExprId, String)> = Vec::new();

    // Walk all top-level stmts (including FnDecl bodies recursively)
    // looking for ObjectLit-field store-sites where the field is
    // tagged Closure and the value is a bare top-FnDecl Ident.
    let stmts_snapshot = ast.stmts.clone();
    for s in &stmts_snapshot {
        crate::ast_collect_fn_closure::collect_fn_to_closure_store_sites(
            ast,
            s,
            &fn_sigs,
            &struct_field_anns,
            &mut targets,
            &mut rewrites,
        );
    }

    if rewrites.is_empty() {
        return;
    }

    // Synthesize one forwarder per unique target (skip if
    // synthesize_forwarders already produced it).
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: HashMap<String, String> = HashMap::new();
    for target in &targets {
        let forward_name = format!("__forward_{target}");
        if existing_forwarders.contains(&forward_name) {
            renames.insert(target.clone(), forward_name);
            continue;
        }
        let (params, return_type) = fn_sigs.get(target).unwrap().clone();
        let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
        fwd_params.push(Param {
            name: "__env".into(),
            type_ann: Some("__env()".to_string()),
            default: None,
            is_rest: false,
        });
        fwd_params.extend(params.iter().cloned());
        let arg_eids: Vec<ExprId> = params
            .iter()
            .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
            .collect();
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
        });
        renames.insert(target.clone(), forward_name);
    }

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
