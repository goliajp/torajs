//! Face-candidate predicates for the fnexpr-this promote pass —
//! carved out of `fnexpr_this.rs` (file-size hard limit) when RFC
//! 20260721 刀 11 G13's knife 6 grew it past 500 prod lines. One
//! face POSITION hands its candidate expression here; the verbatim
//! collect logic (fn-expr `__this` capture / knife-6 method
//! shorthand / knife-2 Ident candidates / literal-descriptor field
//! walk) decides whether it joins the patch list.

use super::sloppy_this_prologue::insert_sloppy_this_prologue;
use super::{Expr, ExprId, Param, Stmt};
use crate::ast::PropKey;

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

/// The `get:` / `set:` / `value:` field values of an INLINE literal
/// descriptor; empty for any non-ObjectLit descriptor expression
/// (variable-routed descriptors alias their faces — knife 2).
///
/// `value:` belongs here for the same reason the accessor halves do.
/// A function installed as a data property is called as a METHOD of
/// whatever it ends up on, so its `this` is the call receiver — not
/// the descriptor literal it was written inside, which is what the
/// object-literal nominal typing would otherwise hand it
/// (`no member .x on Struct([("value", …), ("writable", …)])`).
pub(super) fn literal_desc_faces(exprs: &[Expr], desc: ExprId) -> Vec<ExprId> {
    let Expr::ObjectLit { fields } = &exprs[desc.0 as usize] else {
        return Vec::new();
    };
    fields
        .iter()
        .filter(|(fname, _)| fname == "get" || fname == "set" || fname == "value")
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
    spans: &mut Vec<crate::lexer::Span>,
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
                insert_sloppy_this_prologue(body, exprs, spans);
            }
            fnexpr_recv_fns.insert(fn_name.clone());
            break;
        }
    }
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
        super::fnexpr_this_member_ret::collect_return_faces_in(
            body,
            stmts,
            exprs,
            fn_expr_exprs,
            patches,
        );
    }
}

/// Member-store face — `anyRecv.m = <fn-expr>` (expando method
/// on a wrapper / dynobj receiver). The stored closure is only
/// reachable back through the receiver's props, so every call
/// rides the runtime any-method dispatch, which reads the
/// FLAG_CLOSURE_n header bit and seeds the receiver —
/// zero-alias by construction for a literal fn-expr RHS. An
/// Ident RHS (`o.m = f`) is a face POSITION for knife-2's
/// use-shape analysis: the binding promotes only when its
/// remaining uses are all face reads / direct calls, same bar
/// as every other variable-routed face. (The Index-target
/// twin `o["k"] = fn` rides this arm through the parser's
/// Member desugar for STRING-literal keys; a computed key —
/// `o[Symbol.match] = fn`, `o[k] = fn` — stays Expr::Index
/// and matches the second target pattern: the stored value
/// reaches calls only through the same runtime keyed
/// dispatch, so the face bar is identical.)
///
/// Admitted store receivers:
/// * a runtime-props binding Ident — every declaration of the name
///   is `: any` / `T[]`-annotated, an unannotated `{}` init, or an
///   unannotated array literal (the species-key-2 merged predicate;
///   an array's expando members live in the arrprops bag, the same
///   runtime keyed dispatch the dynobj lane reads);
/// * knife 7 — `F.prototype.m = <fn-expr>` / `F.prototype[k] =
///   <fn-expr>` (the test262 fn-constructor idiom). The prototype
///   object is a runtime dynobj, so the stored closure is reachable
///   only through instances' proto chains and the runtime keyed
///   dispatch. By this pass a fn-decl name in receiver position is
///   already a `__forward_*` Closure wrapper
///   (`synthesize_fn_to_closure_forwarders`), so both spellings of
///   "some named fn's .prototype" admit;
/// * RFC 20260808-construct-channel B2 —
///   `a.constructor[Symbol.species] = <fn>` / `a.constructor.k =
///   <fn>` where `a` is a runtime-props binding (same merged
///   predicate, mutability irrelevant). `.constructor` on those
///   shapes is never a typed struct slot — the stored closure is
///   reachable only through the runtime keyed dispatch
///   (ArraySpeciesCreate reads it back through the arrprops bag).
///   Other member names on these roots stay loud until a consumer
///   shape needs them.
/// * `this.m = <fn-expr>` where the key has no slot the promotion
///   would not survive — declared `any` everywhere, or declared by no
///   nominal type at all. The receiver is the flattened `__this`, and
///   both admissions hand the value back as a NaN box, so every read
///   of it enters the same runtime any lane the receivers above ride
///   ([`super::fnexpr_this_store_fields::ThisStoreKeys`] is what
///   proves the slot).
#[allow(clippy::too_many_arguments)]
pub(super) fn collect_store_face(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_expr_exprs: &std::collections::HashSet<ExprId>,
    props_recvs: &std::collections::HashSet<String>,
    expando_recvs: &super::fnexpr_this_expando::ExpandoRecvs,
    this_store_keys: &super::fnexpr_this_store_fields::ThisStoreKeys,
    target: ExprId,
    value: ExprId,
    patches: &mut Vec<FacePatch>,
    ident_cands: &mut Vec<(String, ExprId)>,
) {
    // The `this.m = fn` arm reads the member NAME, so it is decided on
    // the target rather than on the receiver alone: a computed key
    // (`Expr::Index`) names no field and stays out.
    if let Expr::Member { obj, name } = &exprs[target.0 as usize]
        && matches!(&exprs[peel_any_cast(exprs, *obj).0 as usize], Expr::Ident(n) if n == "__this")
        && this_store_keys.admits(&PropKey::from(name))
    {
        collect_face(stmts, exprs, value, fn_expr_exprs, patches);
        collect_ident_face(exprs, value, ident_cands);
        return;
    }
    // §27.2.4 static-slot patch (rotation 448) — `Promise.resolve =
    // function () { this }` / `.reject`: the store lands in the
    // interned ctor cell's expando dict, and BOTH read-back channels
    // (the any method dispatch and the combinators' patch-consult
    // `invoke_with_this`) shift argv on FLAG_CLOSURE_RECV_FIRST, so a
    // promoted closure reads `this` = the ctor cell on every path —
    // §27.2.4.1.3's `C`. Only the two CONSULTED names admit (the
    // checker's write-face bar), and only while nothing in the
    // program shadows the builtin name.
    if let Expr::Member { obj, name } = &exprs[target.0 as usize]
        && matches!(&exprs[peel_any_cast(exprs, *obj).0 as usize], Expr::Ident(n) if n == "Promise")
        && matches!(name.as_str(), "resolve" | "reject")
        && !super::fnexpr_this_names::name_shadowed_elsewhere(stmts, "Promise")
    {
        collect_face(stmts, exprs, value, fn_expr_exprs, patches);
        collect_ident_face(exprs, value, ident_cands);
        return;
    }
    let store_recv = match &exprs[target.0 as usize] {
        Expr::Member { obj, .. } => Some(*obj),
        Expr::Index { obj, .. } => Some(*obj),
        _ => None,
    };
    let admits = store_recv.is_some_and(|obj| match &exprs[peel_any_cast(exprs, obj).0 as usize] {
        Expr::Ident(n) => props_recvs.contains(n),
        // A keyed hop off one of those receivers is itself in the any
        // world — reading an element of an `any` binding, or of an
        // array whose slots the receiver gate already covers — so a
        // store one level further in (`rows[0][0] = fn`) lands in the
        // same place and comes back through the same channels. What
        // decides is the ROOT the hops start from, so the whole chain
        // peels. Nothing admits here that rotation 592 did not also
        // give a receiver on the way back out: before it, a nested
        // index read seeded none.
        Expr::Index { .. } => index_chain_rooted_in(exprs, obj, props_recvs),
        Expr::Member {
            obj: pobj,
            name: pname,
        } => {
            // `<anything>.prototype.k` — how the prototype object was
            // REACHED does not change the channel the stored value
            // comes back through: an instance method call resolves the
            // name up the prototype chain in the any lane, which shifts
            // argv on FLAG_CLOSURE_RECV_FIRST. The Ident / Closure
            // restriction the first cut carried refused
            // `other.Ctor.prototype.toJSON = function () { ... this ... }`
            // — a member chain, and the ordinary spelling once the
            // constructor lives on a namespace object.
            pname == "prototype"
                || (pname == "constructor"
                    && matches!(&exprs[peel_any_cast(exprs, *pobj).0 as usize], Expr::Ident(n)
                        if props_recvs.contains(n)))
        }
        _ => false,
    });
    // The expando store — a key the receiver's object literal never
    // declared, so the value lands in the dict and comes back through
    // the any lane (doc on `fnexpr_this_expando`).
    if admits || expando_recvs.admits(exprs, target) {
        collect_face(stmts, exprs, value, fn_expr_exprs, patches);
        collect_ident_face(exprs, value, ident_cands);
    }
}

/// Whether a chain of keyed reads starts from one of the receiver
/// bindings whose property slots live in the any world. Each hop is
/// peeled through `as any` for the same reason the single-hop
/// admission is: the cast is typecheck-only and cannot move the
/// value out of that world.
fn index_chain_rooted_in(
    exprs: &[Expr],
    e: ExprId,
    props_recvs: &std::collections::HashSet<String>,
) -> bool {
    let mut cur = peel_any_cast(exprs, e);
    loop {
        match &exprs[cur.0 as usize] {
            Expr::Ident(n) => return props_recvs.contains(n),
            Expr::Index { obj, .. } => cur = peel_any_cast(exprs, *obj),
            _ => return false,
        }
    }
}

/// The store receiver seen through `as any` — a cast the lowering
/// treats as typecheck-only, so it cannot move the stored value out
/// of the any lane every [`collect_store_face`] admission depends on.
///
/// Without this the admissions read the WRAPPER and matched nothing:
/// `Object.prototype.mm = function () { …this… }` promoted while
/// `(Object.prototype as any).mm = …` — the spelling a TS program has
/// to use, since the declared `Object.prototype` type has no such
/// member — took the honest reject. Same for `(C.prototype as any).k`
/// and `(anyBinding as any).f`. Peeling can only widen a receiver
/// toward `any`, never narrow one into a typed slot, so no admission
/// changes meaning: it either sees the shape it was always looking
/// for, or it still sees none.
pub(super) fn peel_any_cast(exprs: &[Expr], mut e: ExprId) -> ExprId {
    while let Expr::As { expr, ty_ann } = &exprs[e.0 as usize] {
        if ty_ann != "any" {
            break;
        }
        e = *expr;
    }
    e
}
