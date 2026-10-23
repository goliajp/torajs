//! Name resolution for the `hidden_any` walk: a name resolves to the
//! innermost declaration in scope, then to the binding a lifted
//! closure's construction site handed it, then by slot key.

use super::super::{Analysis, Scope};
use super::{Graph, Ref};
use crate::check_monomorph_any_widen::struct_escape::twin_original;

impl Graph<'_> {
    /// What `n` names at the current point of the walk.
    pub(in crate::num_width) fn resolve(&self, a: &Analysis<'_>, n: &str, sc: &Scope) -> Ref {
        if let Some(node) = self.frames.iter().rev().find_map(|f| f.get(n)) {
            return Ref::At(node.clone());
        }
        // a closure's all-`any` twin has no construction site of its
        // own; it reads the bindings the original's site names
        let built = twin_original(sc.fn_name).unwrap_or(sc.fn_name);
        if let Some(Some(r)) = self.caps_in.get(&(built.to_string(), n.to_string())) {
            return r.clone();
        }
        if let Some(k) = a.resolve(n, sc) {
            return Ref::Keyed(vec![k]);
        }
        // a lifted body may still write an outer binding of that
        // spelling (`walk::assign_to_name`)
        if sc.fn_name.starts_with("__") {
            return Ref::Keyed(a.named(n).cloned().unwrap_or_default());
        }
        Ref::Keyed(Vec::new())
    }
}
