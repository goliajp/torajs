//! RFC 20260804-method-rebind-generic-body blade 2 — mint the
//! receiver-polymorphic twin `__cmany_<C>__<m>` beside each
//! `__cm_<C>__<m>` instance method whose body reads its receiver.
//!
//! The mono body reads `__this` at static class offsets, which is
//! only sound when the receiver really is a `C` (direct calls,
//! vtable dispatch). A method VALUE that escaped into the any world
//! can be re-bound onto anything (`c.getM().call({v: 42})` —
//! §10.2.1.2 OrdinaryCallBindThis), so the boxed adapter (blade 3)
//! guards on the receiver's `class_tag@+8` and routes a foreign
//! receiver here: same source body, every param (receiver included)
//! typed `any`, so member reads lower through the any-member lane
//! (GetV — reflection reads on a `Tag::Obj`, dynobj reads on a
//! DynObj) instead of baked offsets.
//!
//! Receiver-free bodies mint nothing: the mono body never touches
//! `this`, so it runs correctly under any receiver (the S2.38
//! `this_free` fact, probed here at the AST level off the clone
//! map — an `Expr::This` / `Ident("__this")` among the cloned
//! nodes). A twin-less method keeps today's direct-mono dispatch.
//!
//! Ledger: a dropped mint (receiver-free probe miss, or a super
//! call in the body — see `restore_dynamic_calls`) neutralizes its
//! cloned subtree to `Expr::Uninit` leaves. Plain "unreferenced
//! arena garbage" is NOT inert enough: stmts-walk passes never see
//! it, but whole-arena walks do — `lift_arrow_fns` (`for i in
//! 0..ast.exprs.len()`) lifted a stranded cloned ArrowFn into a
//! `__closure_N` FnDecl whose construction site never lowers, and
//! body lowering then died on the missing capture types
//! (class-super-arrow-001-nested, gate 74e32230).

use super::ast_def::Ast;
use super::clone_body::BodyCloner;
use super::expr::{Expr, Param};
use super::stmt::Stmt;

pub(in crate::ast) fn mint_generic_twin(
    ast: &mut Ast,
    mono_name: &str,
    params: &[Param],
    body: &[Stmt],
    type_params: &[String],
    span: crate::lexer::Span,
    appended: &mut Vec<Stmt>,
) {
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let cloned_params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
            default: p.default.map(|d| cloner.clone_expr(d)),
            is_rest: p.is_rest,
        })
        .collect();
    let reads_receiver = cloner.map.keys().any(|old| {
        matches!(cloner.ast.get_expr(*old), Expr::This | Expr::NewTarget)
            || matches!(cloner.ast.get_expr(*old), Expr::Ident(n) if n == "__this")
    });
    if !reads_receiver {
        neutralize_clone(&mut cloner);
        return;
    }
    super::clone_body_tables::migrate(&mut cloner);
    // The clone happens AFTER Pass 2 / the super rewrite, so the body
    // carries their static forms — sound for the mono (receiver
    // statically this class), unsound under an `any` receiver: the
    // checker's ClassRef arg gate rejects `__cm_<C>__<m>(__this: any)`
    // outright, and spec-wise a method call through a re-bound `this`
    // is a GetV + call (a DynObj receiver's own `m` must shadow the
    // class method). Restore the member-call shape at every
    // this-receiver rewrite site (`cm_this_static_calls` kept the
    // intact Member callee). A static form with NO restore entry is
    // the super rewrite — static binding is CORRECT there (§13.3.7.3
    // HomeObject, receiver-independent) but the mono target's ClassRef
    // receiver param still rejects `any`, so such a method minting no
    // twin is the recorded RFC residue (super-route follow-up blade).
    if !restore_dynamic_calls(&mut cloner) {
        neutralize_clone(&mut cloner);
        return;
    }
    let twin_name = format!("__cmany_{}", &mono_name["__cm_".len()..]);
    ast.cmany_twins
        .insert(mono_name.to_string(), twin_name.clone());
    appended.push(Stmt::FnDecl {
        name: twin_name,
        type_params: type_params.to_vec(),
        params: cloned_params,
        return_type: Some("any".to_string()),
        body: cloned_body,
        is_generator: false,
        span,
    });
}

/// RFC 20260804-fn-this-channel knife 3a — the STATIC-method mirror
/// of [`mint_generic_twin`]: mint `__smany_<C>__<m>` beside
/// `__sm_<C>__<m>` when the static body reads `this`.
///
/// A static body's `this` is the class object at a direct `C.g()`
/// call, and desugar pass 2 rewrote every such site to `Ident(<cls>)`
/// (knife 2 recorded the sites in `ast.static_this_sites`, keyed by
/// the ORIGINAL ExprId — which is exactly what lets this mint tell a
/// receiver read apart from a user-written `C.x`). The twin restores
/// the receiver at those sites: each cloned counterpart becomes
/// `__this`, declared as the leading `any` param, so `C.g.call(recv)`
/// can rebind per §27.7.5.1 — member reads lower through the any
/// lane, and a `this.#f()` rides the private-brand kernels (a
/// receiver without the brand throws, an instance with it runs).
///
/// Mint drops (clone neutralized, mono behavior kept — the recorded
/// RFC residue): a body with no receiver read (`this`-free statics
/// never rebind-misbehave), and a body carrying a static-dispatch
/// call form with no restore record (the super rewrite — see
/// [`restore_dynamic_calls`]; a static-this `this.m()` rewrite
/// records its intact Member as a speculative-demote alt, which the
/// restore step copies back so the twin dispatches dynamically).
pub(in crate::ast) fn mint_static_generic_twin(
    ast: &mut Ast,
    mono_name: &str,
    params: &[Param],
    body: &[Stmt],
    type_params: &[String],
    span: crate::lexer::Span,
    appended: &mut Vec<Stmt>,
) {
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let this_sites: Vec<super::ExprId> = cloner
        .map
        .iter()
        .filter(|(old, _)| cloner.ast.static_this_sites.contains_key(old))
        .map(|(_, new)| *new)
        .collect();
    if this_sites.is_empty() {
        neutralize_clone(&mut cloner);
        return;
    }
    for new in this_sites {
        cloner.ast.exprs[new.0 as usize] = Expr::Ident("__this".into());
    }
    super::clone_body_tables::migrate(&mut cloner);
    // Same restore step as the instance mint: every static-dispatch
    // call form in the clone goes back to its member-call shape (a
    // static body's `this.#f()` recorded its intact Member as a
    // speculative-demote alt — see `restore_dynamic_calls`). A form
    // with no record (the super rewrite) drops the mint.
    if !restore_dynamic_calls(&mut cloner) {
        neutralize_clone(&mut cloner);
        return;
    }
    let mut cloned_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
    cloned_params.push(Param {
        name: "__this".into(),
        type_ann: Some("any".into()),
        default: None,
        is_rest: false,
    });
    cloned_params.extend(params.iter().map(|p| Param {
        name: p.name.clone(),
        type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
        default: p.default.map(|d| cloner.clone_expr(d)),
        is_rest: p.is_rest,
    }));
    let twin_name = format!("__smany_{}", &mono_name["__sm_".len()..]);
    appended.push(Stmt::FnDecl {
        name: twin_name,
        type_params: type_params.to_vec(),
        params: cloned_params,
        return_type: Some("any".to_string()),
        body: cloned_body,
        is_generator: false,
        span,
    });
}

/// Overwrite every cloned node with the inert `Uninit` leaf — a
/// dropped mint must leave NOTHING a whole-arena walk can match (see
/// the module-doc Ledger note; a stranded cloned ArrowFn was lifted
/// into an orphan `__closure_N` whose construction site never
/// lowers). Side-table entries migrated onto these ids stay behind,
/// keyed by ids nothing references — inert by construction.
fn neutralize_clone(cloner: &mut BodyCloner<'_>) {
    let ids: Vec<crate::ast::ExprId> = cloner.map.values().copied().collect();
    for id in ids {
        cloner.ast.exprs[id.0 as usize] = Expr::Uninit;
    }
}

/// Rewrite every cloned static-dispatch call back to its member-call
/// shape (see the call-site comment in [`mint_generic_twin`]). `true`
/// = every static form restored; `false` = a form with no restore
/// entry (the super rewrite) — the caller neutralizes the clone and
/// drops the mint.
fn restore_dynamic_calls(cloner: &mut BodyCloner<'_>) -> bool {
    let new_ids: Vec<crate::ast::ExprId> = cloner.map.values().copied().collect();
    for id in new_ids {
        let Expr::Call { callee, args } = cloner.ast.get_expr(id) else {
            continue;
        };
        let (callee, args) = (*callee, args.clone());
        let Expr::Ident(f) = cloner.ast.get_expr(callee) else {
            continue;
        };
        if !(f.starts_with("__cm_")
            || f.starts_with("__dispatch_")
            || f.starts_with("__supercall__"))
        {
            continue;
        }
        let Some(member) = cloner.ast.cm_this_static_calls.get(&id).copied() else {
            // Knife 3c (RFC 20260804-fn-this-channel) — a receiver
            // that was not `this` at rewrite time (a typed receiver,
            // or a STATIC body's `this` already minted to the class
            // name) recorded its intact member-call shape as a
            // speculative-demote alt instead. The alt Call was
            // migrated alongside the clone, so restoring is copying
            // its shape over the static-dispatch node (its Member
            // callee's `obj` is the cloned receiver read — for a
            // static-this site, the `__this` the mint just wrote).
            if let Some(alt) = cloner.ast.speculative_cm_rewrites.get(&id).copied() {
                let Expr::Call {
                    callee: alt_callee,
                    args: alt_args,
                } = cloner.ast.get_expr(alt).clone()
                else {
                    return false;
                };
                cloner.ast.exprs[id.0 as usize] = Expr::Call {
                    callee: alt_callee,
                    args: alt_args,
                };
                continue;
            }
            return false;
        };
        // Pass 2 prepended the receiver; the intact Member callee's
        // own `obj` (cloned alongside it) carries the receiver read.
        let user_args = args[1..].to_vec();
        cloner.ast.exprs[id.0 as usize] = Expr::Call {
            callee: member,
            args: user_args,
        };
    }
    true
}
