//! Which params a call's arguments land in, for the calls whose param
//! list carries more than the arguments: a receiver, the argv face's
//! synthetic pointer, an accessor write that spells no call at all.

use super::super::{Analysis, Scope, SlotKey};
use crate::ast::ExprId;

impl Analysis<'_> {
    /// A `__cm_` body's params without the argv face's synthetic
    /// pointer: the receiver, then the positions a call fills.
    pub(in crate::num_width) fn method_arg_params(&self, f: &str) -> Option<Vec<String>> {
        let ps = self.fn_params.get(f)?;
        Some(
            ps.iter()
                .filter(|p| *p != "__torajs_argv")
                .cloned()
                .collect(),
        )
    }

    /// Accessor route: `q.X = v` may lower to `__cm_<C>__X_set(q, v)` —
    /// feed the value param of every owner's setter.
    pub(in crate::num_width) fn feed_setter_value(
        &mut self,
        name: &str,
        value: ExprId,
        scope: &Scope,
    ) {
        for c in self.classes.clone() {
            let f = format!("__cm_{c}__{name}_set");
            if let Some(p1) = self.method_arg_params(&f).and_then(|ps| ps.get(1).cloned()) {
                let pk = SlotKey::Param(f.clone(), p1);
                let w = self.flow(value, scope);
                self.add_container_constraint(pk.clone(), w);
                self.alias_guarded(pk, value, scope);
            }
        }
    }
}
