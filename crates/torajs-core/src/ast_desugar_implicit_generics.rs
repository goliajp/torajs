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
//! `infer_return_ann`, `infer_return_ann_seeded`, `AstExprsView`.

use std::collections::HashSet;

use crate::ast::{
    Ast, AstExprsView, Param, Stmt, binds_to_params, body_has_value_return, infer_expr_ann_with,
    infer_return_ann, infer_return_ann_seeded,
};

pub(crate) fn run(ast: &mut Ast) {
    let Ast { stmts, exprs, .. } = ast;
    let ast_exprs_view: AstExprsView = &*exprs;

    let mut fn_sigs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for s in stmts.iter() {
        if let Stmt::FnDecl {
            name,
            return_type: Some(rt),
            type_params,
            ..
        } = s
            && !name.starts_with("__closure_")
            && type_params.is_empty()
        {
            fn_sigs.insert(name.clone(), rt.clone());
        }
    }

    let mut outer_binds: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for s in stmts.iter() {
        if let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        {
            if let Some(ann) = type_ann {
                outer_binds.insert(name.clone(), ann.clone());
            } else {
                let bs: Vec<Param> = binds_to_params(&outer_binds);
                if let Some(ann) =
                    infer_expr_ann_with(ast_exprs_view, *init, &bs, &outer_binds, &fn_sigs)
                {
                    outer_binds.insert(name.clone(), ann);
                }
            }
        }
        if let Stmt::FnDecl { params, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann
                    && p.name != "__env"
                    && p.name != "__this"
                {
                    outer_binds.insert(p.name.clone(), ann.clone());
                }
            }
        }
    }

    // RFC 20260704 C4+ (chunk 522) — pre-infer the lifted-closure
    // signatures so a closure-typed local can bubble out of a fn as
    // its return type. The main loop below visits stmts in source
    // order and the lifted `__closure_*` decls sit at the tail, so a
    // user fn returning a closure local (`const g = (x) => ...;
    // return g`) was inferred BEFORE the closure's own return type
    // existed — the Ident lookup missed and the fn stayed Void.
    // Running the closure branches first (identical, idempotent
    // logic) + publishing each closure's full `__fn(P|..)->R` ann
    // under its reserved `__closure_*` name lets
    // `infer_expr_ann_with`'s Expr::Closure arm answer fn-shaped
    // anns; `parse_type` maps `__fn` to FnSig and `effective_ret_ty`
    // upgrades to Closure where the body returns closure values.
    preinfer_closure_sigs(stmts, ast_exprs_view, &outer_binds, &mut fn_sigs);
    // Second pass over top-level lets — a `const h = <closure>`
    // binding could not resolve before the closure sigs existed.
    for s in stmts.iter() {
        if let Stmt::LetDecl {
            name,
            type_ann: None,
            init,
            ..
        } = s
            && !outer_binds.contains_key(name)
        {
            let bs: Vec<Param> = binds_to_params(&outer_binds);
            if let Some(ann) =
                infer_expr_ann_with(ast_exprs_view, *init, &bs, &outer_binds, &fn_sigs)
            {
                outer_binds.insert(name.clone(), ann);
            }
        }
    }

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
            if first_kind.as_deref() == Some("__env") && name.starts_with("__closure_") {
                for p in params.iter_mut().skip(1) {
                    if p.type_ann.is_none() {
                        p.type_ann = Some("any".to_string());
                    }
                }
            }
            if first_kind.as_deref() == Some("__env")
                && return_type.is_none()
                && body_has_value_return(body)
            {
                if let Some(inferred) =
                    infer_return_ann_seeded(ast_exprs_view, body, params, &outer_binds, &fn_sigs)
                {
                    *return_type = Some(inferred);
                }
            }
            if first_kind.as_deref() == Some("__this")
                && return_type.is_none()
                && body_has_value_return(body)
            {
                if let Some(inferred) =
                    infer_return_ann_seeded(ast_exprs_view, body, params, &outer_binds, &fn_sigs)
                {
                    *return_type = Some(inferred);
                }
            }
            continue;
        }
        if name.starts_with("__closure_") {
            for p in params.iter_mut() {
                if p.type_ann.is_none() && p.name != "__env" && p.name != "__this" {
                    p.type_ann = Some("any".to_string());
                }
            }
            if return_type.is_none() && body_has_value_return(body) {
                if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params, &fn_sigs) {
                    *return_type = Some(inferred);
                }
            }
            continue;
        }

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
            if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params, &fn_sigs) {
                *return_type = Some(inferred);
            }
        }

        if !new_type_params.is_empty() {
            type_params.extend(new_type_params);
        }
    }
}

/// Chunk 522 — run the `__closure_*` param-default + return-sniff
/// branches ahead of the main loop (idempotent with it), then
/// publish each closure's full fn-shaped ann into `fn_sigs` under
/// its reserved name. Closures whose value returns resisted typing
/// publish nothing (no fabricated ann); a body without value
/// returns is `void`.
fn preinfer_closure_sigs(
    stmts: &mut [Stmt],
    exprs: AstExprsView,
    outer_binds: &std::collections::HashMap<String, String>,
    fn_sigs: &mut std::collections::HashMap<String, String>,
) {
    for stmt in stmts.iter_mut() {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = stmt
        else {
            continue;
        };
        if !name.starts_with("__closure_") {
            continue;
        }
        for p in params.iter_mut() {
            if p.type_ann.is_none() && p.name != "__env" && p.name != "__this" {
                p.type_ann = Some("any".to_string());
            }
        }
        if return_type.is_none() && body_has_value_return(body) {
            let has_env = params.first().is_some_and(|p| p.name == "__env");
            let inferred = if has_env {
                infer_return_ann_seeded(exprs, body, params, outer_binds, fn_sigs)
            } else {
                infer_return_ann(exprs, body, params, fn_sigs)
            };
            if let Some(ann) = inferred {
                *return_type = Some(ann);
            }
        }
    }
    for stmt in stmts.iter() {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = stmt
        else {
            continue;
        };
        if !name.starts_with("__closure_") {
            continue;
        }
        let ret = match return_type {
            Some(rt) => rt.clone(),
            None if !body_has_value_return(body) => "void".to_string(),
            None => continue,
        };
        let mut param_anns: Vec<String> = Vec::with_capacity(params.len());
        let mut complete = true;
        for p in params.iter().filter(|p| p.name != "__env") {
            match &p.type_ann {
                Some(a) => param_anns.push(a.clone()),
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if complete {
            fn_sigs.insert(
                name.clone(),
                format!("__fn({})->{}", param_anns.join("|"), ret),
            );
        }
    }
}
