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
use crate::check_pipeline::TypeAliasTables;
use crate::check_type_ann::{str_or_arr_ann, struct_shaped_ann};

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
    if !reads_receiver && !has_lossy_param(cloner.ast, tables, params, false) {
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
    let mut cloner = BodyCloner::new(ast);
    let cloned_body = cloner.clone_stmts(body);
    let reads_this = rebind_this_sites(&mut cloner);
    if !reads_this && !has_lossy_param(cloner.ast, tables, params, true) {
        neutralize_clone(&mut cloner);
        return;
    }
    super::clone_body_tables::migrate(&mut cloner);
    // Same restore step as the instance mint: every static-dispatch
    // call form in the clone goes back to its member-call shape (a
    // static body's `this.#f()` recorded its intact Member as a
    // speculative-demote alt — see `restore_dynamic_calls`). A form
    // with no record (the super rewrite) drops the mint.
    if !restore_dynamic_calls(&mut cloner, params, body) {
        neutralize_clone(&mut cloner);
        return;
    }
    // Rebind AGAIN, because `migrate` may have minted more receiver
    // reads: a speculative alt lives OUTSIDE the body (it hangs off
    // the side table, not off a cloned stmt), so `resolve_value`
    // deep-clones it — and `clone_expr` does not memoize, so the
    // alt's own receiver read becomes a FRESH `Ident(<cls>)` node
    // rather than the one the first pass just rebound. Since
    // `restore_dynamic_calls` installs exactly that alt shape as the
    // twin's call, skipping this second pass leaves the twin reading
    // the class object (`C.f()`) instead of its rebound receiver —
    // every `C.g.call(recv)` then missed, silently for a field read
    // and as "not a function" for a method call.
    rebind_this_sites(&mut cloner);
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
    ast.smany_twins
        .insert(mono_name.to_string(), twin_name.clone());
    if reads_this {
        ast.smany_reads_this.insert(mono_name.to_string());
    }
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

/// A parameter slot a missing argument cannot cross without loss:
/// the boxed adapter unboxes `undefined` into an F64 slot as the
/// NaN sentinel and into a Str slot as the immortal undefined cell,
/// but a `boolean` slot takes `false` and an integer slot `0`, which
/// is a silent wrong answer for `typeof b` / `b === undefined`. A
/// method with such a slot mints its twin even when its body never
/// reads the receiver: the bare call (`const f = d.m; f()`) then
/// runs the twin, whose every parameter is `any`, instead of the
/// loud "class method called without a receiver". A slot with a
/// Number / Bool literal default is exempt — the adapter
/// materializes that default from the same `undefined`; any other
/// default is an expression the adapter cannot replay, so it counts
/// like a lossy slot and the twin evaluates it. An instance method's
/// `number` without a default stays out: a method read as a value
/// already seeds its number parameters F64. A static method has no
/// such seed (`num_width::method_value` walks `__cm_` bodies only),
/// so its integer-valued `number` slot is an I64 and counts. A
/// struct-typed slot counts too: the mono body reads it through the
/// layout, which only a fitting struct cell has, and the adapter
/// sends every other argument — an array, a Proxy, a `3` — to the
/// twin, where `p.x` is the generic [[Get]] (§13.3.2.1). Struct-shaped
/// is the checker's reading of the annotation (`struct_shaped_ann`),
/// so an alias chain, an inline object type, a `P | null` and an array
/// of any of them (`Tok[]`, read element by element through the
/// element layout) all count exactly where the struct-escape lane
/// counts them. A `string` or array slot counts as well: neither holds
/// `null`, `undefined` or another kind of value, which the twin binds
/// as passed (§10.2.1.3).
fn has_lossy_param(ast: &Ast, tables: &TypeAliasTables, params: &[Param], is_static: bool) -> bool {
    params.iter().any(|p| match p.default {
        None => match p.type_ann.as_deref() {
            Some("boolean" | "i64" | "i32") => true,
            Some("number") => is_static,
            // the receiver head carries the class's own annotation
            // and is the mono adapter's env, not an argv slot
            // a typed rest is collected argument by argument, and an
            // argument it cannot hold is the twin's to bind
            Some(ann) if p.is_rest => {
                ann != "any[]" && str_or_arr_ann(ann, &tables.aliases, &tables.generic_alias_decls)
            }
            Some(ann) => {
                p.name != "__this"
                    && (struct_shaped_ann(ann, &tables.aliases, &tables.generic_alias_decls)
                        || str_or_arr_ann(ann, &tables.aliases, &tables.generic_alias_decls))
            }
            None => false,
        },
        // a default that reads `arguments` stays on the direct lane:
        // the adapter's argv carries the declared arity only, so the
        // twin would answer `arguments[3]` as undefined
        Some(d) => {
            !matches!(ast.get_expr(d), Expr::Number(_) | Expr::Bool(_))
                && !super::desugar_async_nested_fns::expr_names_arguments(ast, d)
        }
    })
}

/// Point every cloned counterpart of a static-`this` site at the
/// twin's leading receiver param. `false` = the body reads no
/// receiver at all (a `this`-free static never rebind-misbehaves, so
/// the caller drops the mint and keeps direct-mono dispatch).
///
/// Idempotent, and deliberately re-run after
/// [`super::clone_body_tables::migrate`] — see the second call site.
fn rebind_this_sites(cloner: &mut BodyCloner<'_>) -> bool {
    let sites: Vec<super::ExprId> = cloner
        .map
        .iter()
        .filter(|(old, _)| cloner.ast.static_this_sites.contains_key(old))
        .map(|(_, new)| *new)
        .collect();
    let found = !sites.is_empty();
    for new in sites {
        cloner.ast.exprs[new.0 as usize] = Expr::Ident("__this".into());
    }
    found
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
