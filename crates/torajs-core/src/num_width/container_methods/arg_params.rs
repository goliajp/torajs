//! Which params a call's arguments land in, for the calls whose param
//! list carries more than the arguments: a receiver, the argv face's
//! synthetic pointer, an accessor write that spells no call at all.

use super::super::{Analysis, Scope};
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

    /// The params of closure `fname` (the value `eid`) a call's arguments
    /// land in. An object-literal method or accessor is called with its
    /// receiver threaded apart and, on the argv face, the argv pointer
    /// filled by the ABI: `__p{i}` is the i-th argument the caller
    /// writes, which neither of those is. A promoted function
    /// expression's receiver is threaded apart the same way.
    pub(in crate::num_width) fn closure_arg_params(&self, fname: &str, eid: ExprId) -> Vec<String> {
        let objlit_method = self.ast.objlit_method_exprs.contains(&eid);
        let recv = objlit_method || self.ast.fnexpr_recv_fns.contains(fname);
        let mut ps = self.user_params(fname);
        ps.retain(|p| !(recv && p == "__this") && !(objlit_method && p == "__torajs_argv"));
        ps
    }

    /// Accessor route: `q.X = v` may lower to `__cm_<C>__X_set(q, v)` —
    /// feed the value param of every owner's setter — or, on an object
    /// literal, call the closure in its `__setter_X` slot, whose first
    /// argument projection the literal's fill glued to that param.
    pub(in crate::num_width) fn feed_setter_value(
        &mut self,
        obj: ExprId,
        name: &str,
        value: ExprId,
        scope: &Scope,
    ) {
        if let Some(rk) = self.container_key_of(obj, scope) {
            let slot = self.keys.field(rk, format!("__setter_{name}"));
            let pk = self.keys.field(slot, "__p0");
            self.mark_containerish(pk);
            let w = self.flow(value, scope);
            self.add_container_constraint(pk, w);
        }
        for c in self.classes.clone() {
            let f = format!("__cm_{c}__{name}_set");
            if let Some(p1) = self.method_arg_params(&f).and_then(|ps| ps.get(1).cloned()) {
                let pk = self.keys.param(&f, &p1);
                let w = self.flow(value, scope);
                self.add_container_constraint(pk, w);
                self.alias_guarded(pk, value, scope);
            }
        }
    }
}
