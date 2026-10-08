//! Both layers of the lattice note what each admitted pairing lets
//! flow into an interface (`crate::iface_witness::note_flow`) at every
//! depth they reach: whether an interface keeps its witness slots is
//! settled from those notes after the check.

use super::{deep_inner, shallow_inner};
use crate::check::{GenericAliasMap, Type};

pub(crate) fn is_assignable_to_deep(
    to: &Type,
    from: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
    seen: &mut std::collections::HashSet<(String, String)>,
) -> bool {
    let fits = deep_inner(to, from, class_structs, aliases, generic_aliases, seen);
    crate::iface_witness::note_flow(to, from, fits);
    fits
}

pub(crate) fn is_assignable_to(to: &Type, from: &Type) -> bool {
    let fits = shallow_inner(to, from);
    crate::iface_witness::note_flow(to, from, fits);
    fits
}
