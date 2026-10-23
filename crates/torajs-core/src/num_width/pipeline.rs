//! The W4 container pipeline the `analyze` driver runs between the
//! walk and the fixpoint: nominal class hookups, guarded-union
//! activation, container-channel merge, congruence closure, then
//! rewrite of every seed / edge key onto its alias-class
//! representative. The fixpoint runs per alias class; queries
//! canonicalize through the frozen union-find.

use super::{Analysis, container, promise_taint, slot_abi};

/// Join and freeze the alias classes, then seed the number faces under
/// a promise that a promise can reach unseen; answers what lowering's
/// reads ask (`promise_taint`).
pub(super) fn join_classes(a: &mut Analysis) -> promise_taint::PromiseTaint {
    container::nominal_unions(a);
    a.join_bound_candidates();
    container::objlit_ctor_unions(a);
    container::steps::generator_step_unions(a);
    container::steps::iterator_step_unions(a);
    slot_abi::dispatch_unions(a);
    slot_abi::relay_unions(a);
    a.alias_nominal_unions();
    a.fnsig_nominal_unions();
    container::activate_guarded(a);
    a.seeds.append(&mut a.c_seeds);
    for (d, ts) in std::mem::take(&mut a.c_edges) {
        a.edges.entry(d).or_default().extend(ts);
    }
    container::canonicalize(a);
    promise_taint::seed_untracked_points(a)
}
