//! Face-candidate predicates for the fnexpr-this promote pass —
//! carved out of `fnexpr_this.rs` (file-size hard limit) when RFC
//! 20260721 刀 11 G13's knife 6 grew it past 500 prod lines. One
//! face POSITION hands its candidate expression here; the verbatim
//! collect logic (fn-expr `__this` capture / knife-6 method
//! shorthand / knife-2 Ident candidates / literal-descriptor field
//! walk) decides whether it joins the patch list.

use super::{Expr, ExprId, Stmt};

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
