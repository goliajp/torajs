//! RFC 20260717-fnexpr-this-channel knife 1 — give a function
//! expression sitting in an INLINE accessor-face position its
//! call-site `this`.
//!
//! `o.__defineSetter__("y", function (v) { this._y = v })` — the
//! parser encodes the fn-expr as an `Expr::ArrowFn` (marked in
//! `ast.fn_expr_exprs`), `desugar_classes` pass 2 rewrites the body's
//! `this` to a bare `__this` Ident, and `lift_arrow_fns` lists it as a
//! capture — which the checker rejects (`__this` is unbound at the
//! definition scope). ES §10.2.1.2 binds a function expression's
//! `this` at the CALL SITE; for an accessor face that is the property
//! read/write receiver.
//!
//! This pass is the fn-expr mirror of `objlit_nominal::apply_patches`
//! (RFC 20260714-objlit-accessor blade 1): drop `__this` from the
//! closure's capture list (it is a receiver, not a capture), insert it
//! as the first declared param after `__env` (typed `any` — the
//! receiver arrives as a NaN-box through the boxed dual entry), and
//! record the lifted fn name in `ast.fnexpr_recv_fns` so the closure
//! construction site stamps `FLAG_CLOSURE_RECV_FIRST` and the
//! accessor-face lowering marks the AccessorPair kinds byte
//! `ACC_KIND_RECV`.
//!
//! ONLY inline positions promote — the fn-expr must itself be the
//! face argument/field, so the closure value has zero aliases and no
//! receiver-unaware call path can reach it (a promoted body's native
//! signature has an extra leading param; a plain `f(x)` call would
//! shift every argument — the exact silent-wrong B-4 narrow-surface
//! forbids). Covered positions:
//!
//! * `recv.__defineGetter__(k, <fn-expr>)` / `__defineSetter__` —
//!   annex B §B.2.2.2 legacy define (argument 1);
//! * `Object.defineProperty(o, k, { get: <fn-expr>, set: <fn-expr> })`
//!   — the literal descriptor's face fields.
//!
//! Everything else (variable-routed faces, direct calls, callback
//! `thisArg`, `defineProperties` / `Object.create` nesting) keeps
//! today's loud checker reject; RFC knives 2-5.
//!
//! Runs inside `desugar_implicit_generics` right after
//! `objlit_nominal::run` — `lift_arrow_fns` has produced the
//! `Expr::Closure` nodes and `preinfer_closure_sigs` has already
//! published the user-facing `__fn(P)->R` anns (which deliberately
//! stay `__this`-free, like `__mth(`'s receiver-less spelling).

use super::{Expr, ExprId, Param, Stmt};

/// A closure to patch: the lifted FnDecl gains a `__this: any` param.
struct FacePatch {
    eid: ExprId,
    fn_name: String,
}

pub(crate) fn run(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
) {
    if fn_expr_exprs.is_empty() {
        return;
    }
    let mut patches: Vec<FacePatch> = Vec::new();
    for i in 0..exprs.len() {
        let Expr::Call { callee, args } = &exprs[i] else {
            continue;
        };
        match &exprs[callee.0 as usize] {
            // `recv.__defineGetter__(k, face)` / `__defineSetter__`.
            Expr::Member { name, .. }
                if name == "__defineGetter__" || name == "__defineSetter__" =>
            {
                if let Some(face) = args.get(1) {
                    collect_face(exprs, *face, fn_expr_exprs, &mut patches);
                }
            }
            // `Object.defineProperty(o, k, { get: face, set: face })` —
            // the descriptor must be an INLINE object literal; a
            // variable-routed descriptor aliases its faces (knife 2).
            Expr::Member { obj, name } if name == "defineProperty" => {
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(n) if n == "Object") {
                    continue;
                }
                let Some(desc) = args.get(2) else { continue };
                let Expr::ObjectLit { fields } = &exprs[desc.0 as usize] else {
                    continue;
                };
                let faces: Vec<ExprId> = fields
                    .iter()
                    .filter(|(fname, _)| fname == "get" || fname == "set")
                    .map(|(_, feid)| *feid)
                    .collect();
                for face in faces {
                    collect_face(exprs, face, fn_expr_exprs, &mut patches);
                }
            }
            _ => {}
        }
    }
    if patches.is_empty() {
        return;
    }
    // `__this` is a receiver, not a capture — mirror
    // `objlit_nominal::apply_patches` exactly: retain the capture list,
    // then rewrite the lifted FnDecl's params + `__env(...)` ann.
    for p in &patches {
        if let Expr::Closure { captures, .. } = &mut exprs[p.eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }
    for p in &patches {
        let caps: Vec<String> = match &exprs[p.eid.0 as usize] {
            Expr::Closure { captures, .. } => captures.clone(),
            _ => continue,
        };
        for s in stmts.iter_mut() {
            let Stmt::FnDecl { name, params, .. } = s else {
                continue;
            };
            if *name != p.fn_name {
                continue;
            }
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
                        type_ann: Some("any".to_string()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
            fnexpr_recv_fns.insert(p.fn_name.clone());
            break;
        }
    }
}

/// A face candidate promotes when it is a marked fn-expr Closure whose
/// body actually says `this` (pass-2 left it in the capture list). A
/// `this`-free fn-expr face keeps the plain closure ABI — receiverless
/// invoke stays byte-identical.
fn collect_face(
    exprs: &[Expr],
    face: ExprId,
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    if !fn_expr_exprs.contains(&face) {
        return;
    }
    let Expr::Closure { fn_name, captures } = &exprs[face.0 as usize] else {
        return;
    };
    if !captures.iter().any(|c| c == "__this") {
        return;
    }
    patches.push(FacePatch {
        eid: face,
        fn_name: fn_name.clone(),
    });
}
