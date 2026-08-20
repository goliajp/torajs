//! The dynobj-DEGRADED leg of the anylane objlit widen — a literal
//! whose binding a LATER statement pushes onto the dynobj lane.
//!
//! `collect_anylane_objlits`' legs (a)-(h) all read the literal's OWN
//! site: an `any` annotation on it, an `as any` around it, a
//! computed key inside it, an argument position under it. None of
//! them can see the other direction — a plain `var o = { get 0() {…}
//! }` that only becomes a dynobj two statements later, when
//! `Object.defineProperty(o, …)` / `delete o.x` / `o[k] = v` converts
//! the runtime cell. The accessor face was stamped `__this:
//! __ObjLit_n` in the meantime, and `guard_anylane_recv_face` rejects
//! it loudly at the dynobj init (24 test262 cases across 16
//! directories, rotation 461).
//!
//! The trigger set is NOT re-derived here: [`crate::dynobj_degrade`]
//! already owns it, and both the checker (binding type) and the
//! lowerer (init-lane routing) query the same collector — a
//! hand-rolled second copy is the drift that module's doc bans. This
//! leg runs it against the desugar-phase snapshot through
//! `DegradeView`.
//!
//! Marking alone would be silent-wrong: the invariant
//! `widen_detached_method_objlits` states is that the marked set must
//! stay ⊆ the literals that actually lower dynobj, and a mark that
//! the checker does not follow hands an `any`-receiver body a struct
//! pointer. So this leg widens the DECLARATION the same way that
//! knife does — `type_ann = any`, what the user could have written —
//! and check, lower and the (a) leg then all follow from one AST
//! fact. The degrade collector reaches the same conclusion from the
//! same triggers, so the annotation only makes explicit a lane the
//! binding was already taking.
//!
//! Restricted to ACCESSOR-bearing literals: those are the only faces
//! `guard_anylane_recv_face` rejects (a plain method keeps the closure
//! ABI), and a degraded data-only literal already lowers correctly
//! without the annotation.

use std::collections::{HashMap, HashSet};

use super::{Expr, ExprId, Stmt};
use crate::dynobj_degrade::DegradeView;

pub(super) fn widen_degraded_accessor_objlits(
    stmts: &mut [Stmt],
    exprs: &[Expr],
    objlit_computed_keys: &HashMap<ExprId, ExprId>,
    objlit_computed_accessors: &HashMap<ExprId, bool>,
) {
    let degraded: HashSet<ExprId> = crate::dynobj_degrade::collect_degraded_inits(DegradeView {
        stmts,
        exprs,
        objlit_computed_keys,
    });
    if degraded.is_empty() {
        return;
    }
    let admit = |_name: &str, init: ExprId| {
        degraded.contains(&init) && has_accessor_field(exprs, init, objlit_computed_accessors)
    };
    super::objlit_nominal_anylane::widen_inner(stmts, &admit);
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
