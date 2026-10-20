//! The W4 container pipeline the `analyze` driver runs between the
//! walk and the fixpoint: nominal class hookups, guarded-union
//! activation, container-channel merge, congruence closure, then
//! rewrite of every seed / edge key onto its alias-class
//! representative. The fixpoint runs per alias class; queries
//! canonicalize through the frozen union-find.

use std::collections::HashSet;

use super::{Analysis, SlotKey, container, promise_entry, slot_abi};

/// Join and freeze the alias classes, then seed the promise value
/// points a promise can reach unseen; answers the classes that reach
/// them (`promise_entry`).
pub(super) fn join_classes(a: &mut Analysis) -> HashSet<SlotKey> {
    container::nominal_unions(a);
    a.join_bound_candidates();
    container::objlit_ctor_unions(a);
    container::generator_step_unions(a);
    slot_abi::dispatch_unions(a);
    slot_abi::relay_unions(a);
    a.alias_nominal_unions();
    a.fnsig_nominal_unions();
    // every alias pair the walk recorded, before activation drops the
    // ones no container evidence joins
    let alias_pairs = if a.promises {
        [&a.guarded_unions[..], &a.nested_unions[..]].concat()
    } else {
        Vec::new()
    };
    container::activate_guarded(a);
    a.seeds.append(&mut a.c_seeds);
    for (d, ts) in std::mem::take(&mut a.c_edges) {
        a.edges.entry(d).or_default().extend(ts);
    }
    container::canonicalize(a);
    promise_entry::seed_untracked_points(a, &alias_pairs)
}
