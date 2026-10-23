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
//! A body that reads `arguments` mints one whatever it reads: the
//! mono answers for one argument count, and the twin, on the argv
//! face, hosts every boxed call that passes another.
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
use super::desugar_classes_ctor_twin::has_lossy_param;
use super::expr::{Expr, Param};
use super::stmt::Stmt;
use crate::check_pipeline::TypeAliasTables;

mod static_twin;
pub(in crate::ast) use static_twin::mint_static_generic_twin;

/// The desugared method a twin is minted from.
pub(in crate::ast) struct TwinSource<'s> {
    pub(in crate::ast) mono_name: &'s str,
    pub(in crate::ast) params: &'s [Param],
    pub(in crate::ast) body: &'s [Stmt],
    pub(in crate::ast) type_params: &'s [String],
    pub(in crate::ast) span: crate::lexer::Span,
}

pub(in crate::ast) fn mint_generic_twin(
    ast: &mut Ast,
    tables: &TypeAliasTables,
    src: TwinSource<'_>,
    appended: &mut Vec<Stmt>,
) {
    let TwinSource {
        mono_name,
        params,
        body,
        type_params,
        span,
    } = src;
    let reads_arguments = super::arguments_object_walkers::body_has_any_arguments_touch(ast, body);
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let cloned_params: Vec<Param> = params
        .iter()
        .map(|p| Param {
            name: p.name.clone(),
            type_ann: Some(if p.is_rest { "any[]" } else { "any" }.to_string()),
            // an `any` slot binds an absent argument as undefined by
            // itself; dropping the optional's implicit default keeps an
            // arguments-reading twin on the argv face, which refuses
            // defaults
            default: p
                .default
                .filter(|&d| !(reads_arguments && super::binds_undefined(cloner.ast, d)))
                .map(|d| cloner.clone_expr(d)),
            is_rest: p.is_rest,
        })
        .collect();
    let reads_receiver = cloner.map.keys().any(|old| {
        matches!(cloner.ast.get_expr(*old), Expr::This | Expr::NewTarget)
            || matches!(cloner.ast.get_expr(*old), Expr::Ident(n) if n == "__this")
    });
    // a body that reads `arguments` mints one too: its argv twin hosts
    // the boxed calls whose count the mono's folded answer misses
    if !reads_receiver && !has_lossy_param(cloner.ast, tables, params, false) && !reads_arguments {
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
    if !restore_dynamic_calls(&mut cloner, params, body) {
        neutralize_clone(&mut cloner);
        return;
    }
    let twin_name = format!("__cmany_{}", &mono_name["__cm_".len()..]);
    ast.cmany_twins
        .insert(mono_name.to_string(), twin_name.clone());
    super::desugar_async_ssa_gate::mark_twin(ast, mono_name, &twin_name);
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
pub(super) fn neutralize_clone(cloner: &mut BodyCloner<'_>) {
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
pub(super) fn restore_dynamic_calls(
    cloner: &mut BodyCloner<'_>,
    params: &[Param],
    body: &[Stmt],
) -> bool {
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
                // The twin retypes only what flows from its receiver
                // and its parameters. A receiver the method never
                // binds — a module-level `let g = new Box()` read from
                // the body — keeps its static class in the twin too,
                // and the member-call shape has no lane for a typed
                // receiver (every lane past the static one expects
                // `any`): `class K { m() { this.x; return g.get() } }`
                // died in lowering with "unsupported member call
                // shape", and so did every generator body doing the
                // same, since `__Gen_<f>.next` always reads `this`.
                // Static dispatch is exactly right for such a
                // receiver, so the rewrite stands.
                if receiver_keeps_static_type(cloner.ast, alt_callee, params, body) {
                    continue;
                }
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

/// Is the member call's receiver a bare name the method neither takes
/// as a parameter nor binds anywhere in its body — a free variable
/// whose static type the twin leaves alone? `this`, `__this` and
/// every parameter are retyped `any` in the twin; a local may inherit
/// that through its initializer, so any name the body binds answers
/// `false` (the conservative direction: restoring keeps today's path).
fn receiver_keeps_static_type(
    ast: &Ast,
    callee: crate::ast::ExprId,
    params: &[Param],
    body: &[Stmt],
) -> bool {
    let Expr::Member { obj, .. } = ast.get_expr(callee) else {
        return false;
    };
    // A fresh instance — pass 2 spelled `new C(…)` as a call to the
    // factory `__new_C` — is of the class the factory names whatever
    // the twin retypes: its arguments may read `this`, its class does
    // not. Restoring the member shape here left `new C().k(3)` inside
    // any `this`-reading method (a generator's `next` included, so
    // every async body moved into one) with no lane past the static
    // one ("unsupported member call shape: k", rotation 642).
    if let Expr::Call { callee: f, .. } = ast.get_expr(*obj)
        && let Expr::Ident(f) = ast.get_expr(*f)
        && f.starts_with("__new_")
        && f != "__new_target"
    {
        return true;
    }
    let Expr::Ident(name) = ast.get_expr(*obj) else {
        return false;
    };
    // A static body's `this.h()` reached pass 2 already spelled as
    // `<Class>.h()` (knife 2), so its alt's receiver is the class
    // name — the one free name the twin DOES rebind (`rebind_this_sites`
    // turns it into `__this` after this step).
    name != "__this"
        && !ast.class_parents.contains_key(name)
        && !params.iter().any(|p| p.name == *name)
        && !body
            .iter()
            .any(|s| super::desugar_generators_alpha::rebinds_in_stmt(s, name))
}
