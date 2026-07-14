//! RFC 20260714-objlit-accessor blade 1 — give a method-bearing object
//! literal a nominal identity so its methods can bind `this`.
//!
//! `{ a: 5, m() { return this.a * 2 } }` did not compile: `this` inside
//! a method body is rewritten to `Ident("__this")` by
//! `desugar_classes_pass2` (which walks the WHOLE expr arena, on the
//! assumption — stated in its own comment — that `this` only ever
//! appears inside class methods), and nothing binds `__this` at an
//! object literal. `free_vars_of_arrow` then collects it as a capture
//! and `check_closure` rejects it: "closure `__closure_0` references
//! unknown identifier `__this`".
//!
//! The fix mirrors what classes already do. A class method's `__this`
//! param is annotated with the CLASS NAME (`desugar_classes_emit`), and
//! the class itself is turned into a `Stmt::TypeDecl` whose fields are
//! the DATA fields only (`desugar_classes_pass3`); the checker's
//! TypeDecl pre-pass (`check_pipeline`) resolves that into
//! `aliases[C] = Struct(..)` before any function body is checked. So
//! `__this: C` resolves, and there is no recursion — because methods
//! are NOT layout fields, the struct type never mentions itself.
//!
//! We mint the same shape for an object literal: a synthetic nominal
//! alias `__ObjLit_<n>` plus a `TypeDecl` carrying its data fields. The
//! one thing a class method cannot do and an object-literal method must
//! is CAPTURE (`function f() { let c = 0; return { bump() { return ++c
//! } } }`), so the lifted FnDecl keeps its `__env` and takes `__this` as
//! a second param: `(__env, __this, ...user)`.
//!
//! The field annotation is minted as `__mth(<recv>|<user params>)->R`.
//! The marker rides the prefix (the `__clsargc(` idiom from RFC
//! 20260708): `parse_type` decodes it exactly like `__cls(` so the SSA
//! sig carries the receiver and `CallIndirect`'s argv lines up, while
//! the checker drops the leading receiver so `o.m(x)` types with the
//! arity the user wrote.
//!
//! Runs INSIDE `desugar_implicit_generics`, after `preinfer_closure_sigs`
//! and before its main loop: the lifted closures exist by then (so we
//! can patch their FnDecls) and `fn_sigs` is populated (so we can type
//! the data fields), but the object literal's own `__inlobj(...)` ann
//! has not been minted yet — which is what lets our `__mth(` field ann
//! flow out through the normal return-type inference.

use std::collections::HashMap;

use super::{AstExprsView, Expr, ExprId, Param, Stmt, binds_to_params, infer_expr_ann_with};

/// One method field of one object literal, resolved to the pieces the
/// apply phase needs. Collected read-only so the arena walk and the
/// arena mutation don't overlap.
struct MethodPatch {
    /// The `Expr::Closure` slot (same ExprId the `ArrowFn` had).
    eid: ExprId,
    /// The lifted FnDecl to give a `__this` param.
    fn_name: String,
    /// The object literal's synthetic nominal alias.
    objlit_ty: String,
    /// The field name this method sits under.
    field: String,
}

pub(crate) fn run(
    stmts: &mut Vec<Stmt>,
    exprs: &mut Vec<Expr>,
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    objlit_method_fields: &mut HashMap<String, Vec<String>>,
    outer_binds: &HashMap<String, String>,
    fn_sigs: &mut HashMap<String, String>,
) {
    if objlit_method_exprs.is_empty() {
        return;
    }
    let mut type_decls: Vec<Stmt> = Vec::new();
    let mut patches: Vec<MethodPatch> = Vec::new();

    {
        let view: AstExprsView = &*exprs;
        let bind_params = binds_to_params(outer_binds);
        let mut next = 0usize;
        for i in 0..view.len() {
            let Expr::ObjectLit { fields } = &view[i] else {
                continue;
            };
            if !fields.iter().any(|(_, e)| objlit_method_exprs.contains(e)) {
                continue;
            }
            let objlit_ty = format!("__ObjLit_{next}");
            next += 1;

            // METHODS ARE LAYOUT FIELDS — they are own enumerable
            // properties, and `this.other()` has to resolve against the
            // receiver's own type. That is safe only because a `__mth(`
            // slot's signature does NOT name the receiver: were it in
            // there, `__ObjLit_n`'s layout would refer to itself, and
            // `parse_struct` has no memo-before-fill to break the cycle.
            //
            // A data field the ann sniffer can't type falls back to
            // `any` rather than dropping out of the layout, so
            // `this.<field>` still resolves.
            let mut td_fields: Vec<(String, String)> = Vec::new();
            let mut method_names: Vec<String> = Vec::new();
            for (fname, feid) in fields {
                if objlit_method_exprs.contains(feid) {
                    let Expr::Closure { fn_name, .. } = &view[feid.0 as usize] else {
                        // Not lifted (`lift_arrow_fns` runs first, so
                        // this shouldn't happen). Leave it be rather
                        // than guess a shape.
                        continue;
                    };
                    method_names.push(fname.clone());
                    td_fields.push((fname.clone(), "__mth_placeholder".to_string()));
                    patches.push(MethodPatch {
                        eid: *feid,
                        fn_name: fn_name.clone(),
                        objlit_ty: objlit_ty.clone(),
                        field: fname.clone(),
                    });
                    continue;
                }
                let ann = infer_expr_ann_with(view, *feid, &bind_params, outer_binds, fn_sigs)
                    .unwrap_or_else(|| "any".to_string());
                td_fields.push((fname.clone(), super::retag_field_fn_ann(&ann)));
            }
            objlit_method_fields.insert(objlit_ty.clone(), method_names);
            type_decls.push(Stmt::TypeDecl {
                name: objlit_ty,
                type_params: Vec::new(),
                fields: td_fields,
            });
        }
    }

    if patches.is_empty() {
        return;
    }

    // `__this` is a receiver, not a capture. It only landed in the
    // capture list because pass-2 rewrote `this` to a bare Ident before
    // anyone knew where it was bound.
    for p in &patches {
        if let Expr::Closure { captures, .. } = &mut exprs[p.eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }

    for p in &patches {
        let Some(caps) = closure_captures(exprs, p.eid) else {
            continue;
        };
        let mut mth_ann: Option<String> = None;
        for s in stmts.iter_mut() {
            let Stmt::FnDecl {
                name,
                params,
                return_type,
                ..
            } = s
            else {
                continue;
            };
            if *name != p.fn_name {
                continue;
            }
            // The `__env(...)` ann lists the captured names; `__this` is
            // out of the capture list now, so it has to leave the ann
            // too — otherwise the lowerer reads an env slot nothing
            // stored.
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({})", caps.join("|")));
            }
            if !params.iter().any(|q| q.name == "__this") {
                let at = usize::from(params.first().is_some_and(|q| q.name == "__env"));
                params.insert(
                    at,
                    Param {
                        name: "__this".to_string(),
                        // The NOMINAL alias — same shape as a class
                        // method's `__this: C`. Resolving it gives the
                        // body typed field reads AND sibling-method
                        // calls (`this.other()`).
                        type_ann: Some(p.objlit_ty.clone()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
            // Re-publish the sig: `preinfer_closure_sigs` ran before
            // `__this` existed and spells every closure `__fn(`. The
            // `__mth(` params are the USER params only — the receiver
            // must stay out of every Type (see the module doc).
            let ret = return_type.clone().unwrap_or_else(|| "void".to_string());
            let param_anns: Vec<String> = params
                .iter()
                .filter(|q| q.name != "__env" && q.name != "__this")
                .map(|q| q.type_ann.clone().unwrap_or_else(|| "any".to_string()))
                .collect();
            let ann = format!("__mth({})->{}", param_anns.join("|"), ret);
            fn_sigs.insert(p.fn_name.clone(), ann.clone());
            mth_ann = Some(ann);
            break;
        }
        // Fill the placeholder the collect phase parked in the TypeDecl:
        // the ann needs the FnDecl's params, which only exist post-patch.
        if let Some(ann) = mth_ann {
            for td in type_decls.iter_mut() {
                let Stmt::TypeDecl { name, fields, .. } = td else {
                    continue;
                };
                if *name != p.objlit_ty {
                    continue;
                }
                for (fname, fty) in fields.iter_mut() {
                    if *fname == p.field {
                        *fty = ann.clone();
                    }
                }
                break;
            }
        }
    }

    stmts.extend(type_decls);
}

fn closure_captures(exprs: &[Expr], eid: ExprId) -> Option<Vec<String>> {
    match &exprs[eid.0 as usize] {
        Expr::Closure { captures, .. } => Some(captures.clone()),
        _ => None,
    }
}
