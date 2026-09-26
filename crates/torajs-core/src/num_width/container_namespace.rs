//! Member calls on a builtin namespace receiver (`Promise.*`,
//! `Array.from`, `Object.defineProperty`): the receiver is no
//! container, so each wires its own edges. Split from
//! `container_methods.rs` (file-size limit).

use super::{Analysis, Scope};
use crate::ast::{Expr, ExprId};

impl<'a> Analysis<'a> {
    /// A call on a builtin namespace receiver (`Promise.*`,
    /// `Array.from` / `<TypedArray>.from`, `Object.defineProperty(ies)`)
    /// — the receiver is no container, so each wires its own edges.
    /// `true` when the call was one of them.
    pub(super) fn namespace_call_effects(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        name: &str,
        args: &[ExprId],
        scope: &Scope,
    ) -> bool {
        // ②.6b — Promise statics have an untrackable receiver (the
        // global namespace ident); their value-point wiring must fire
        // from here, the every-call-site hook (the result-key path
        // only fires when the result is consumed via a tracked flow —
        // `console.log(await Promise.resolve(2.5))` never is).
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Promise"
            && self.resolve(ns, scope).is_none()
        {
            self.promise_static_wiring(eid, name, args, scope);
            return true;
        }
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && (ns == "Array"
                || crate::ast::desugar_classes_builtin_heritage::is_typedarray_ctor(ns))
            && name == "from"
            && self.resolve(ns, scope).is_none()
        {
            self.array_from_wiring(args, scope);
            return true;
        }
        // W-ESC (RFC 20260721-object-descriptor-cluster 刀 5) — the
        // Object.defineProperty / defineProperties receiver escapes to
        // the any world: the define family's exotic semantics
        // (attribute shadows, accessor indexes, cross-kind values)
        // exist only on the NaN-box lane, and a typed receiver's
        // raw-slot loads could not observe them. The runtime typed arm
        // keeps its loud reject as the fallback for receiver shapes
        // this analysis can't see.
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Object"
            && self.resolve(ns, scope).is_none()
            && matches!(name, "defineProperty" | "defineProperties")
        {
            if let Some(a0) = args.first()
                && let Some(rk) = self.container_key_of(*a0, scope)
            {
                self.any_seeds.push(rk);
            }
            return true;
        }
        false
    }
}
