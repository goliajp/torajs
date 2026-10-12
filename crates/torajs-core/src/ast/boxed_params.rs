//! The boxed-entry census as the checker and the lowerer both read it.
//!
//! The census records a generic's call sites by the generic's own name,
//! while a typed specialization (`<g>$$_<types>`) is checked and lowered
//! under its own; a census entry may also be written under the
//! specialization. Both readers take the union of the two, so the
//! checker types a call through the parameter by the value's return
//! exactly where the lowerer routes it through the boxed entry.

use std::collections::HashSet;

use super::Ast;

impl Ast {
    /// The parameters of `name` whose direct calls ride the boxed entry.
    pub(crate) fn boxed_params_of(&self, name: &str) -> Option<HashSet<String>> {
        let own = self.argv_boxed_params.get(name);
        let generic = spec_generic(name).and_then(|g| self.argv_boxed_params.get(g));
        match (own, generic) {
            (None, None) => None,
            (Some(s), None) | (None, Some(s)) => Some(s.clone()),
            (Some(a), Some(b)) => Some(a.union(b).cloned().collect()),
        }
    }
}

/// The generic a typed specialization `<g>$$_<types>` instantiates.
pub(crate) fn spec_generic(name: &str) -> Option<&str> {
    name.rsplit_once("$$")
        .filter(|(_, tail)| tail.starts_with('_'))
        .map(|(g, _)| g)
}
