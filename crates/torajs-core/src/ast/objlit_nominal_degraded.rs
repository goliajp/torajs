//! The dynobj-DEGRADED leg of [`super::objlit_nominal_anylane`] — a
//! literal whose binding a LATER statement pushes onto the dynobj
//! lane.
//!
//! Legs (a)-(h) all read the literal's OWN site: an `any` annotation
//! on it, an `as any` around it, a computed key inside it, an
//! argument position under it. None of them can see the other
//! direction — a plain `var o = { get 0() {…} }` that only becomes a
//! dynobj two statements later, when `Object.defineProperty(o, …)` /
//! `delete o.x` / `o[k] = v` converts the runtime cell. The accessor
//! face was stamped `__this: __ObjLit_n` in the meantime, and
//! `guard_anylane_recv_face` rejected it loudly (24 test262 cases
//! across 16 directories, rotation 461).
//!
//! The trigger set is NOT re-derived here: [`crate::dynobj_degrade`]
//! already owns it, and both the checker (binding type) and the
//! lowerer (init-lane routing) query the same collector — a
//! hand-rolled second copy is the drift that module's doc bans. This
//! leg runs it against the desugar-phase snapshot through
//! `DegradeView`, which is also what keeps the marked set ⊆ the
//! literals that really lower dynobj (the invariant
//! `widen_detached_method_objlits` states).
//!
//! MARKING, not widening. The first version of this leg annotated the
//! declaration `: any` instead — the (a)-leg spelling the detached-
//! method knife uses — and that is self-defeating HERE: the degrade
//! collector only tracks UNANNOTATED lets, so writing the annotation
//! takes the binding straight back out of the trigger set every
//! consumer downstream recomputes. Measured on
//! `with/get-mutable-binding-binding-deleted-in-get-unscopables`: the
//! deleted property read back `undefined` instead of raising the
//! §9.1.1.2 ReferenceError.
//!
//! Restricted to literals carrying an ACCESSOR or a METHOD. The
//! accessor half is what `guard_anylane_recv_face` rejects loudly.
//! The method half was assumed correct with its nominal stamp intact
//! and is not: a plain `var a = { k: 1, method() { return this } }`
//! whose binding a later `Object.setPrototypeOf(a, …)` degrades kept
//! the `__this: __ObjLit_n` stamp while the binding lowered dynobj,
//! so the receiver arriving at the method was a struct-unbox COPY —
//! `a.method() === a` answered FALSE with no diagnostic. That is the
//! same "receiver writes land on a copy / identity-loss" family the
//! (b)/(g) legs of `objlit_nominal_anylane` already name; those legs
//! read the literal's own site and this one is the late-degrade
//! direction of it.
//!
//! A method that never touches `this` would be safe either way, but
//! the set is not narrowed on that: over-broad `any` is slower and
//! never wrong, which is the posture `dynobj_degrade`'s own
//! conservative fallback takes for the same reason.

use std::collections::{HashMap, HashSet};

use super::{Expr, ExprId, Stmt};
use crate::dynobj_degrade::DegradeView;

pub(super) fn degraded_recv_face_objlits(
    stmts: &[Stmt],
    exprs: &[Expr],
    objlit_method_exprs: &HashSet<ExprId>,
    objlit_computed_keys: &HashMap<ExprId, ExprId>,
    objlit_computed_accessors: &HashMap<ExprId, bool>,
) -> Vec<ExprId> {
    let degraded: HashSet<ExprId> = crate::dynobj_degrade::collect_degraded_inits(DegradeView {
        stmts,
        exprs,
        objlit_computed_keys,
    });
    degraded
        .into_iter()
        .filter(|e| {
            has_accessor_field(exprs, *e, objlit_computed_accessors)
                || has_method_field(exprs, *e, objlit_method_exprs)
        })
        .collect()
}

/// Does the literal carry a method shorthand? The parser records
/// every one in `objlit_method_exprs` keyed by its VALUE expression,
/// which is the same set `objlit_nominal::run` gates its whole pass
/// on.
fn has_method_field(exprs: &[Expr], init: ExprId, objlit_method_exprs: &HashSet<ExprId>) -> bool {
    let Expr::ObjectLit { fields } = &exprs[init.0 as usize] else {
        return false;
    };
    fields
        .iter()
        .any(|(_, fe)| objlit_method_exprs.contains(fe))
}

/// Does the literal carry an accessor member? The shorthand spelling
/// parses to a `__getter_x` / `__setter_x` field; a computed one
/// (`get [k]() {}`) keeps the `__computed_N__` sentinel name and is
/// recorded in the accessor side-table instead.
fn has_accessor_field(
    exprs: &[Expr],
    init: ExprId,
    objlit_computed_accessors: &HashMap<ExprId, bool>,
) -> bool {
    let Expr::ObjectLit { fields } = &exprs[init.0 as usize] else {
        return false;
    };
    fields.iter().any(|(n, fe)| {
        n.starts_with("__getter_")
            || n.starts_with("__setter_")
            || objlit_computed_accessors.contains_key(fe)
    })
}
