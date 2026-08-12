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
    exprs: &mut Vec<Expr>,
    patches: &[(ExprId, String)],
    fnexpr_recv_fns: &mut std::collections::HashSet<String>,
    sloppy: bool,
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
            let Stmt::FnDecl {
                name, params, body, ..
            } = s
            else {
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
                // `[__env?, __torajs_argv?, __this?]` (RFC 20260808
                // knife 2's recv-slot shape) — an argv-face body
                // promoted here AFTER the arguments pass must keep
                // its argv slot ahead of the receiver, or the
                // adapter reads it as a user param and the body
                // derefs a boxed value as the argv pointer (probe:
                // SIGSEGV).
                let mut at = usize::from(params.first().is_some_and(|q| q.name == "__env"));
                while params.get(at).is_some_and(|q| q.name == "__torajs_argv") {
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
            if sloppy {
                insert_sloppy_this_prologue(body, exprs);
            }
            fnexpr_recv_fns.insert(fn_name.clone());
            break;
        }
    }
}

/// §10.2.1.2 OrdinaryCallBindThis step 6 (ThisMode ~global~) — under
/// the sloppy script goal a promoted body's `this` is the GLOBAL
/// OBJECT whenever the call site supplied no receiver (undefined /
/// null in the `__this` slot). The binding is a callee-side prologue
/// (`__this = __this ?? globalThis`), the textbook place: every call
/// lane — boxed dispatch, HOF loop seed, direct-call seed — flows
/// through the body, so one rewrite covers them all, and strict-goal
/// files never enter (their detached `this` stays undefined per the
/// step-5 strict branch). Idempotent by shape probe: a body whose
/// first stmt already assigns `__this` was patched by an earlier
/// promote round (the objlit_nominal → fnexpr_this fall-through
/// pair). A primitive receiver keeps its unwrapped value — the
/// step-4 ToObject wrapper face is recorded residue (L3b), not a
/// silent wrap here.
fn insert_sloppy_this_prologue(body: &mut Vec<Stmt>, exprs: &mut Vec<Expr>) {
    if let Some(Stmt::Expr(e)) = body.first()
        && let Expr::Assign { target, .. } = &exprs[e.0 as usize]
        && matches!(&exprs[target.0 as usize], Expr::Ident(n) if n == "__this")
    {
        return;
    }
    let target = ExprId(exprs.len() as u32);
    exprs.push(Expr::Ident("__this".to_string()));
    let lhs = ExprId(exprs.len() as u32);
    exprs.push(Expr::Ident("__this".to_string()));
    let rhs = ExprId(exprs.len() as u32);
    exprs.push(Expr::Ident("globalThis".to_string()));
    let nullish = ExprId(exprs.len() as u32);
    exprs.push(Expr::Nullish { lhs, rhs });
    let assign = ExprId(exprs.len() as u32);
    exprs.push(Expr::Assign {
        target,
        value: nullish,
    });
    body.insert(0, Stmt::Expr(assign));
}

/// Rotation 375 — a marked fn-expr standing as a `throw` operand
/// (`throw function () { …this… }`, the try-statement 12.14 family).
/// The thrown value crosses into the exception channel, which is
/// `any`-shaped end to end: a `catch (e)` binding is `any`, so every
/// downstream consumption — `e()`, `e.m`, a re-throw — rides the
/// runtime any lane, whose call paths all honor
/// FLAG_CLOSURE_RECV_FIRST (`__torajs_any_call` →
/// `invoke_with_this`, detached receiver `undefined` per §10.2.1.2 —
/// the sloppy prologue then answers globalThis). Zero aliases: the
/// fn-expr is the throw operand itself. Walks every statement list
/// through the shared spine, FnDecl bodies included (a `throw`
/// inside a lifted closure body is the common spelling).
pub(super) fn collect_throw_faces(
    body: &[Stmt],
    all_stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    for s in body {
        if let Stmt::Throw(e) = s {
            collect_face(all_stmts, exprs, *e, fn_expr_exprs, patches);
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_throw_faces(inner, all_stmts, exprs, fn_expr_exprs, patches)
        });
    }
}

/// Rotation 346 — the seventh receiver-safe face position: a marked
/// fn-expr RETURNED from an object-literal method or accessor body
/// (`get f() { return function () { …this… } }`, the await-using
/// dispose-getter family). The value leaves through the method's
/// return channel, and a method/accessor result is consumed on the
/// any lane (the accessor protocol / method dispatch), where every
/// call path is receiver-flag-aware — no receiver-unaware call can
/// reach it (rotation 345's any-lane anchor). Zero aliases: the
/// fn-expr is the return operand itself. Without the promote the
/// inner `__this` stayed a capture the method had to supply —
/// receiver-correct only when the method receiver and the eventual
/// call-site receiver happened to be the same object — and the
/// pre-promote capture snapshot rode every enclosing lift as a stale
/// `__this` (the `await-using` unknown-identifier reject family).
pub(super) fn collect_method_return_faces(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    objlit_method_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    let method_fns: std::collections::HashSet<&str> = objlit_method_exprs
        .iter()
        .filter_map(|eid| match &exprs[eid.0 as usize] {
            Expr::Closure { fn_name, .. } => Some(fn_name.as_str()),
            _ => None,
        })
        .collect();
    if method_fns.is_empty() {
        return;
    }
    for s in stmts {
        let Stmt::FnDecl { name, body, .. } = s else {
            continue;
        };
        if !method_fns.contains(name.as_str()) {
            continue;
        }
        collect_return_faces_in(body, stmts, exprs, fn_expr_exprs, patches);
    }
}

/// Every `return <marked fn-expr>` in the METHOD's own body — nested
/// compound statements walk through the shared spine, but a nested
/// `function` declaration is its own return scope and is skipped.
fn collect_return_faces_in(
    body: &[Stmt],
    all_stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    patches: &mut Vec<FacePatch>,
) {
    for s in body {
        if let Stmt::Return(Some(e)) = s {
            collect_face(all_stmts, exprs, *e, fn_expr_exprs, patches);
        }
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_return_faces_in(inner, all_stmts, exprs, fn_expr_exprs, patches)
        });
    }
}
