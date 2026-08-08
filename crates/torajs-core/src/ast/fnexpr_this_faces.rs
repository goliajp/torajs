//! Face-candidate predicates for the fnexpr-this promote pass —
//! carved out of `fnexpr_this.rs` (file-size hard limit) when RFC
//! 20260721 刀 11 G13's knife 6 grew it past 500 prod lines. One
//! face POSITION hands its candidate expression here; the verbatim
//! collect logic (fn-expr `__this` capture / knife-6 method
//! shorthand / knife-2 Ident candidates / literal-descriptor field
//! walk) decides whether it joins the patch list.

use super::{Expr, ExprId, Param, Stmt};

/// A closure to patch: the lifted FnDecl gains a `__this: any` param.
pub(super) struct FacePatch {
    pub(super) eid: ExprId,
    pub(super) fn_name: String,
}

/// A face candidate promotes when it is a marked fn-expr Closure whose
/// body actually says `this` (pass-2 left it in the capture list). A
/// `this`-free fn-expr face keeps the plain closure ABI — receiverless
/// invoke stays byte-identical.
///
/// Knife 6 (RFC 20260721 刀 11 G13) — a method-SHORTHAND face
/// (`Object.defineProperty(o, k, { get() { …this… } })`): the "get"
/// field is an object-literal method, so `objlit_nominal` already
/// gave its lifted FnDecl a `__this` param typed with the literal's
/// nominal alias — but §10.1.7.3 binds an accessor face's `this` to
/// the property READ receiver, not the descriptor object. The face
/// position has zero aliases (inline literal, defineProperty
/// consumes it), so the promote re-anns `__this` to `any` and marks
/// recv-first; a `this`-free shorthand has no `__this` param and
/// stays on the plain ABI.
pub(super) fn collect_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    face: ExprId,
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    let Expr::Closure { fn_name, captures } = &exprs[face.0 as usize] else {
        return;
    };
    if fn_expr_exprs.contains(&face) {
        if captures.iter().any(|c| c == "__this") {
            patches.push(FacePatch {
                eid: face,
                fn_name: fn_name.clone(),
            });
            return;
        }
        // RFC 20260725 — fn-expr FIELDS join `objlit_method_exprs`,
        // so `objlit_nominal` may have promoted this face already
        // (dropped `__this` from the captures, gave the lifted
        // FnDecl a `__this` param). Fall through to the shorthand's
        // has-this-param probe so the knife-6 re-ann (accessor
        // `this` is the property READ receiver, `any`-typed) applies
        // to the fn-expr spelling too.
    }
    let has_this_param = stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.iter().any(|q| q.name == "__this"))
    });
    if has_this_param {
        patches.push(FacePatch {
            eid: face,
            fn_name: fn_name.clone(),
        });
    }
}

/// Record a face-position `Expr::Ident` as a knife-2 candidate —
/// resolution (single-use + const fn-expr init) happens after the
/// position walk.
pub(super) fn collect_ident_face(exprs: &[Expr], face: ExprId, cands: &mut Vec<(String, ExprId)>) {
    if let Expr::Ident(n) = &exprs[face.0 as usize] {
        cands.push((n.clone(), face));
    }
}

/// The `get:` / `set:` field values of an INLINE literal descriptor;
/// empty for any non-ObjectLit descriptor expression (variable-routed
/// descriptors alias their faces — knife 2).
pub(super) fn literal_desc_faces(exprs: &[Expr], desc: ExprId) -> Vec<ExprId> {
    let Expr::ObjectLit { fields } = &exprs[desc.0 as usize] else {
        return Vec::new();
    };
    fields
        .iter()
        .filter(|(fname, _)| fname == "get" || fname == "set")
        .map(|(_, feid)| *feid)
        .collect()
}

/// Promote each `(closure eid, lifted fn name)` to the receiver-first
/// any shape: `__this` leaves the capture list (it is a receiver, not
/// a capture — mirror of `objlit_nominal::apply_patches`), the lifted
/// FnDecl gains a `__this: any` param right after `__env`, and the fn
/// name joins `fnexpr_recv_fns` so the construction site stamps
/// `FLAG_CLOSURE_RECV_FIRST` and receiver-aware invokers put the
/// receiver in argv[0]. Shared by this pass's fn-expr faces and
/// `objlit_nominal`'s any-lane literal members (RFC
/// 20260717-objlit-anylane-recv knife 1).
pub(crate) fn promote_recv_any(
    stmts: &mut [Stmt],
    exprs: &mut [Expr],
    patches: &[(ExprId, String)],
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
) {
    for (eid, _) in patches {
        if let Expr::Closure { captures, .. } = &mut exprs[eid.0 as usize] {
            captures.retain(|c| c != "__this");
        }
    }
    for (eid, fn_name) in patches {
        let caps: Vec<String> = match &exprs[eid.0 as usize] {
            Expr::Closure { captures, .. } => captures.clone(),
            _ => continue,
        };
        for s in stmts.iter_mut() {
            let Stmt::FnDecl { name, params, .. } = s else {
                continue;
            };
            if name != fn_name {
                continue;
            }
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({})", caps.join("|")));
            }
            if let Some(t) = params.iter_mut().find(|q| q.name == "__this") {
                // Knife 6 — a method-shorthand face arrives with a
                // nominal-typed `__this` (objlit_nominal's receiver);
                // the accessor invoke hands a NaN-box, so the param
                // re-anns to `any` (typeof / member reads go dynamic).
                t.type_ann = Some("any".to_string());
            } else {
                // The boxed-entry adapter reads the head as
                // `[__env?, __torajs_real_argc?, __torajs_argv?,
                // __this?]` (RFC 20260808 knife 2's recv-slot shape)
                // — an argv-face body promoted here AFTER the
                // arguments pass must keep its argc/argv slots ahead
                // of the receiver, or the adapter reads them as user
                // params and the body derefs a boxed value as the
                // argv pointer (probe: SIGSEGV).
                let mut at = usize::from(params.first().is_some_and(|q| q.name == "__env"));
                while params
                    .get(at)
                    .is_some_and(|q| q.name == "__torajs_real_argc" || q.name == "__torajs_argv")
                {
                    at += 1;
                }
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
            fnexpr_recv_fns.insert(fn_name.clone());
            break;
        }
    }
}
