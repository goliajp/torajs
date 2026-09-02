//! The closure-shaped arms of
//! [`crate::ast_desugar_implicit_generics`] — split to this sibling
//! when the fall-through-return gate (rotation 272) pushed the main
//! file past the 500-line limit (the rotation-213 watch predicted
//! the move). Two arms + their shared seed-map builder, verbatim:
//!
//! - `desugar_closure_shape_fn` — `__env` / `__this`-first FnDecls
//!   (lifted arrows, forwarder shims, desugared class methods).
//! - `desugar_lifted_closure_fn` — `__closure_*` names without the
//!   `__env`-first shape.
//!
//! Both default un-annotated params to `any` and infer /
//! `any`-fallback the return type; callers live in the main file's
//! FnDecl loop and `preinfer_closure_sigs`.

use crate::ast::{
    AstExprsView, Param, PropKey, Stmt, body_has_value_return, infer_return_ann,
    infer_return_ann_seeded,
};
use crate::ast_desugar_implicit_generics::is_synth_closure_name;

pub(crate) fn desugar_closure_shape_fn(
    first_kind: Option<&str>,
    name: &str,
    params: &mut Vec<Param>,
    return_type: &mut Option<String>,
    body: &[Stmt],
    ast_exprs_view: AstExprsView,
    outer_binds: &std::collections::HashMap<String, String>,
    cap_anns: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    receiver_fields: &std::collections::HashMap<String, Vec<(PropKey, String)>>,
    fn_sigs: &mut std::collections::HashMap<String, String>,
) {
    if (first_kind == Some("__env") && is_synth_closure_name(name)) || first_kind == Some("__this")
    {
        // `__this`-first = desugared class methods (incl. synthesized
        // `__param_destr_N` holders) + bind_this_param-promoted fns —
        // implicit-any surfaces per TS noImplicitAny=false.
        for p in params.iter_mut().skip(1) {
            if p.type_ann.is_none() {
                p.type_ann = Some("any".to_string());
            }
        }
    }
    if first_kind == Some("__env") && return_type.is_none() && body_has_value_return(body) {
        let seeded = seeded_binds_for(name, outer_binds, cap_anns);
        if let Some(inferred) =
            infer_return_ann_seeded(ast_exprs_view, body, params, &seeded, fn_sigs)
        {
            *return_type = Some(inferred);
        } else if is_synth_closure_name(name) {
            // RC-4 — a value return the static sniff can't
            // type (any-param method call / any arith) must
            // not silently become Void: the callee then
            // DROPPED its return value and every call read 0.
            // Fall back to `any`, mirroring the param default.
            *return_type = Some("any".to_string());
        }
    }
    if first_kind == Some("__this") && return_type.is_none() && body_has_value_return(body) {
        let mut seeded = seeded_binds_for(name, outer_binds, cap_anns);
        // r502 — the receiver's declared fields, so `return this.v`
        // types as `v` does (TS infers a method's return from its
        // body; the class row is the environment the shape grammar
        // lacked). Only for a body that cannot fall off its end: a
        // reachable fall-through answers `undefined`, which a scalar
        // slot cannot spell (the fn-decl arm's SIGTRAP lesson) — that
        // shape keeps taking the `any` floor below. A generic
        // receiver's field anns name its type params, meaningless
        // as a return ann; an exotic-parent receiver is `any`.
        if crate::ast::body_always_terminates(body)
            && let Some(fields) = receiver_fields_of(params, receiver_fields)
        {
            // Seeds are keyed by the `this.<field>` Member spelling,
            // which only an identifier-shaped key can have.
            for (field, ann) in fields {
                if let Some(f) = field.as_str() {
                    seeded.insert(format!("this.{f}"), ann.clone());
                }
            }
        }
        if let Some(inferred) =
            infer_return_ann_seeded(ast_exprs_view, body, params, &seeded, fn_sigs)
        {
            // r502 — publish it the way the fn-decl arm does, so a
            // method emitted later in source order (a subclass's
            // `total() { return this.sum() + this.w }` — the call is
            // already spelled `__cm_<Owner>__sum`) can sniff the call.
            if !inferred.starts_with("__T") {
                fn_sigs.insert(name.to_string(), inferred.clone());
            }
            *return_type = Some(inferred);
        } else {
            // The same fallback the other two arms take, for the same
            // reason — this arm is the one that never got it. The sniff
            // is a shape grammar with no type environment past the param
            // annotations, so a field read off the receiver defeats it:
            // `class C { v = 5; read() { return this.v; } }` left the
            // return None, and the checker then expected Void and
            // rejected the method. Reading a field is most of what
            // methods do, so the arm without the fallback was turning
            // away a large share of ordinary classes.
            //
            // `any` is the floor, not the goal — the receiver's class
            // does know `v: number`, and teaching the sniff to read it
            // would keep the method on the typed lane instead of boxing
            // every return. That is additive; this restores the
            // programs, it does not settle their representation.
            *return_type = Some("any".to_string());
        }
    }
}

/// r502 — every class the class desugar flattened into a struct
/// decl, by name: a `__this`-shaped method's return sniff resolves
/// `this.<field>` against its receiver's row.
pub(crate) fn receiver_field_rows(
    stmts: &[Stmt],
) -> std::collections::HashMap<String, Vec<(PropKey, String)>> {
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, fields, .. } => Some((name.clone(), fields.clone())),
            _ => None,
        })
        .collect()
}

/// The receiver's field rows for a `__this`-first fn whose receiver
/// ann is a plain class name (`C`, not `C<T>` / `any`).
fn receiver_fields_of<'a>(
    params: &[Param],
    receiver_fields: &'a std::collections::HashMap<String, Vec<(PropKey, String)>>,
) -> Option<&'a Vec<(PropKey, String)>> {
    let ann = params.first()?.type_ann.as_deref()?;
    if ann.contains('<') {
        return None;
    }
    receiver_fields.get(ann)
}

/// Main-loop lifted-closure arm (`__closure_*` name without the
/// `__env`-first shape) — default un-annotated params to `any` and
/// infer / `any`-fallback the return type.
pub(crate) fn desugar_lifted_closure_fn(
    params: &mut Vec<Param>,
    return_type: &mut Option<String>,
    body: &[Stmt],
    ast_exprs_view: AstExprsView,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    for p in params.iter_mut() {
        if p.type_ann.is_none() && p.name != "__env" && p.name != "__this" {
            p.type_ann = Some("any".to_string());
        }
    }
    if return_type.is_none() && body_has_value_return(body) {
        if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params, fn_sigs) {
            *return_type = Some(inferred);
        } else {
            // RC-4 — un-typeable value return falls back to
            // `any` instead of Void (see the __env arm above).
            *return_type = Some("any".to_string());
        }
    }
}

/// Seed map for one closure's return-ann sniff: the construction-site
/// capture snapshot overlays the program-wide by-name fallback.
/// Also consumed by the main file's `preinfer_closure_sigs`.
pub(crate) fn seeded_binds_for(
    name: &str,
    outer_binds: &std::collections::HashMap<String, String>,
    cap_anns: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
) -> std::collections::HashMap<String, String> {
    let mut merged = outer_binds.clone();
    if let Some(snap) = cap_anns.get(name) {
        merged.extend(snap.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    merged
}

pub(crate) fn preinfer_closure_sigs(
    stmts: &mut [Stmt],
    exprs: AstExprsView,
    outer_binds: &std::collections::HashMap<String, String>,
    cap_anns: &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    argv_fns: &std::collections::HashSet<String>,
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
                let seeded = seeded_binds_for(name, outer_binds, cap_anns);
                infer_return_ann_seeded(exprs, body, params, &seeded, fn_sigs)
            } else {
                infer_return_ann(exprs, body, params, fn_sigs)
            };
            if let Some(ann) = inferred {
                *return_type = Some(ann);
            } else {
                // RC-4 — un-typeable value return falls back to `any`
                // instead of Void (dropped return, every call read 0).
                *return_type = Some("any".to_string());
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
        if !is_synth_closure_name(name) {
            continue;
        }
        let ret = match return_type {
            Some(rt) => rt.clone(),
            None if !body_has_value_return(body) => "void".to_string(),
            None => continue,
        };
        // r454 — an argv-face member's real head is the boxed
        // `[__torajs_argv, …]` shape; walking its params would stamp
        // the opaque `__argvptr()` into every inferred ann that
        // carries this sig (an enclosing fn returning the closure
        // most of all). Publish the same rest-tail spelling the
        // checker gives the closure VALUE, so consumers route the
        // variadic boxed lane.
        if argv_fns.contains(name) {
            fn_sigs.insert(
                name.clone(),
                crate::type_ann_fnsig::fn_type_ann("__fn", "__rest(any[])", &ret),
            );
            continue;
        }
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
                crate::type_ann_fnsig::fn_type_ann("__fn", &param_anns.join("|"), &ret),
            );
        }
    }
}
