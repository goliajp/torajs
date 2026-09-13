//! The runtime-props receiver census — the B2 store-receiver bar.
//!
//! Moved out of [`super::fnexpr_this_recvs`] verbatim in rotation 594:
//! that file stood at 478 prod lines, and this census then needed an
//! arm for a CALL initializer, which does not fit under the 500-line
//! limit alongside the five other censuses.

use super::fnexpr_this_faces::peel_any_cast;
use super::{Expr, ExprId, Stmt};

/// Runtime-props receiver bindings for the B2 store-receiver bar
/// (RFC 20260808-construct-channel, species key 2) — a name admits
/// when its EVERY declaration is one of the runtime-props shapes:
/// an `: any` / `T[]`-annotated binding, an unannotated `{}` init,
/// or an unannotated array-literal init. One walk over BOTH shape
/// families replaces the retired lenient array collector: test262
/// harness helpers commonly declare `const a: any` while the case
/// body declares `var a = []` under the same name, and two
/// separately-walked collectors each classified the other's shape
/// as "other" — the over-removal killed the name in both sets, so
/// the store arm never admitted the case's
/// `a.constructor[Symbol.species]` store. The store-receiver
/// argument only needs "this root's stored face is a runtime props
/// slot, never a typed struct field", which every one of these
/// shapes preserves across a `var` reassignment (the checker's type
/// discipline keeps an array-typed binding an array). The knife-4
/// HOF collectors keep their stricter bars — their consumers pair a
/// receiver VALUE with a callback, where a rebind changes which
/// cell the promoted callback meets.
pub(super) fn collect_props_receiver_binding_names(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<String> {
    let mut fn_rets: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    collect_fn_return_anns(stmts, &mut fn_rets);
    let mut names = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    collect_props_receiver_inner(stmts, exprs, &fn_rets, &mut names, &mut other);
    names.retain(|n| !other.contains(n));
    names
}

/// Every function declaration's declared return annotation, by name,
/// at any depth. Read only to answer what shape a CALL initializer
/// hands its binding — see the `Expr::Call` arm below. Program-wide
/// by name, the same approximation grade as every census in this
/// family; a name two declarations disagree about is dropped rather
/// than guessed at.
fn collect_fn_return_anns(stmts: &[Stmt], out: &mut std::collections::HashMap<String, String>) {
    let mut conflicting: Vec<String> = Vec::new();
    walk(stmts, out, &mut conflicting);
    for n in conflicting {
        out.remove(&n);
    }

    fn walk(
        stmts: &[Stmt],
        out: &mut std::collections::HashMap<String, String>,
        conflicting: &mut Vec<String>,
    ) {
        for s in stmts {
            if let Stmt::FnDecl {
                name, return_type, ..
            } = s
            {
                match return_type {
                    Some(rt) => {
                        if out
                            .insert(name.clone(), rt.clone())
                            .is_some_and(|p| p != *rt)
                        {
                            conflicting.push(name.clone());
                        }
                    }
                    None => conflicting.push(name.clone()),
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                walk(inner, out, conflicting)
            });
        }
    }
}

fn collect_props_receiver_inner(
    stmts: &[Stmt],
    exprs: &[Expr],
    fn_rets: &std::collections::HashMap<String, String>,
    names: &mut std::collections::HashSet<String>,
    other: &mut std::collections::HashSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                let props_shape = match type_ann.as_deref() {
                    Some(a) => a == "any" || a.ends_with("[]"),
                    None => match &exprs[init.0 as usize] {
                        Expr::Array(_) => true,
                        Expr::ObjectLit { fields } => fields.is_empty(),
                        // Rotation 594 — a CALL initializer. The
                        // binding's shape is the callee's DECLARED
                        // return annotation, read by the same bar the
                        // annotated arm above uses. Without it
                        // `const z = mk()` was pushed into `other` and
                        // REMOVED the name, so `z[0] = fn` lost its
                        // receiver while the byte-identical program
                        // spelled `const z: any[][] = [[]]` kept it —
                        // a difference no one writing either could
                        // predict.
                        Expr::Call { callee, .. } => match &exprs[callee.0 as usize] {
                            Expr::Ident(f) => fn_rets
                                .get(f)
                                .is_some_and(|a| a == "any" || a.ends_with("[]")),
                            _ => false,
                        },
                        _ => false,
                    },
                };
                if props_shape {
                    names.insert(name.clone());
                } else {
                    other.insert(name.clone());
                }
            }
            // Rotation 645 — a function's PARAMETERS are bindings too,
            // by the same bar: one annotated `any` (or an array) keeps
            // its properties in the any world, so a store into it
            // (`function observe(object: any, name) { object[name] =
            // function () { …arguments… } }` — the test262 harness's
            // `observeMethod`) lands in a runtime props slot and the
            // stored fn's calls come back through the boxed entry.
            // Without the params the census only ever saw declared
            // bindings, and the stored fn's `arguments` stayed an
            // unknown identifier. Any other annotation, or none, is a
            // typed slot: the name goes to `other` like a typed `let`.
            Stmt::FnDecl { params, .. } => {
                for p in params {
                    if p.name.starts_with("__") {
                        continue;
                    }
                    let props_shape = p
                        .type_ann
                        .as_deref()
                        .is_some_and(|a| a == "any" || a.ends_with("[]"));
                    if props_shape {
                        names.insert(p.name.clone());
                    } else {
                        other.insert(p.name.clone());
                    }
                }
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
            collect_props_receiver_inner(inner, exprs, fn_rets, names, other)
        });
    }
}

/// Whether a RECEIVER holds its properties in the any world — the half
/// of [`super::fnexpr_this_faces::store_target_admits`] that looks only
/// at what the value is being put into, with no opinion about the key.
///
/// It lives here rather than beside that caller because this is the
/// same bar the census above answers, one level further out: that one
/// says which BINDINGS keep their properties in the any world, this one
/// says which EXPRESSIONS reach such a binding.
///
/// Split out in rotation 595 for the push/unshift arm: handing a fn to
/// `z.push(...)` parks it in the very slot `z[0] = ...` would have
/// used, so the question "does a value stored here come back through a
/// receiver-honouring channel" is the same question and must not get a
/// second, drifting answer. The key-shaped arms of that function
/// (`this.m`, `Promise.resolve`, the expando dict) stay behind — they
/// are decided on the member NAME, which an argument position has none
/// of.
pub(super) fn store_receiver_admits(
    exprs: &[Expr],
    props_recvs: &std::collections::HashSet<String>,
    obj: ExprId,
) -> bool {
    match &exprs[peel_any_cast(exprs, obj).0 as usize] {
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
        Expr::Index { .. } => keyed_chain_rooted_in(exprs, obj, props_recvs),
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
            // Rotation 597 — a NAMED hop off one of those receivers is
            // the same situation as the keyed hop above, and was the
            // only one of the two the chain test did not peel:
            // `bag.fns.push(fn)` where `bag` is an any-world binding
            // parks the value in a slot reached by an any-lane read of
            // `bag`, so it comes back out through the any lane exactly
            // as `rows[0].push(fn)` does. Measured before the arm: the
            // binding spelling `const z: any[] = []` answered the
            // holder while `bag.fns` answered `undefined`, for the same
            // program.
            pname == "prototype"
                || (pname == "constructor"
                    && matches!(&exprs[peel_any_cast(exprs, *pobj).0 as usize], Expr::Ident(n)
                        if props_recvs.contains(n)))
                || keyed_chain_rooted_in(exprs, obj, props_recvs)
        }
        _ => false,
    }
}

/// Whether a chain of property reads starts from one of the receiver
/// bindings whose property slots live in the any world. Each hop is
/// peeled through `as any` for the same reason the single-hop
/// admission is: the cast is typecheck-only and cannot move the value
/// out of that world.
///
/// Both hop spellings peel, and for one reason: what decides is the
/// ROOT, not how the chain spells its way down from it. `rows[0][0]`
/// and `bag.fns` are the same read of the same any-world slot, and a
/// value stored through either comes back through the same channel.
/// Until rotation 597 only the keyed spelling peeled, so the two
/// halves of one rule disagreed about the same program.
fn keyed_chain_rooted_in(
    exprs: &[Expr],
    e: ExprId,
    props_recvs: &std::collections::HashSet<String>,
) -> bool {
    let mut cur = peel_any_cast(exprs, e);
    loop {
        match &exprs[cur.0 as usize] {
            Expr::Ident(n) => return props_recvs.contains(n),
            Expr::Index { obj, .. } | Expr::Member { obj, .. } => cur = peel_any_cast(exprs, *obj),
            _ => return false,
        }
    }
}
