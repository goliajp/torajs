//! W4 — read-only container-key resolution: the `&self` mirror of
//! `container_walk.rs`'s flow-site walkers, consumed by `width_of`
//! (which cannot take the side-effecting `&mut` path; the walk has
//! already attached Anon constraints and unions at flow sites).

use super::{Analysis, Scope, SlotKeyId};
use crate::ast::{BinOp, Expr, ExprId};

impl<'a> Analysis<'a> {
    /// Read-only mirror of `container_key_of` — same key for every
    /// shape, none of the side effects (Anon constraints, unions).
    /// `width_of` consumes this: it runs under `&self` and the walk
    /// has already attached the side effects at flow sites.
    pub(super) fn container_key_lookup(&self, eid: ExprId, scope: &Scope) -> Option<SlotKeyId> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) => {
                if let Some(k) = self.resolve(n, scope) {
                    return Some(k);
                }
                if scope.fn_name.starts_with("__") && self.named(n).is_some() {
                    return Some(self.keys.captured(n));
                }
                None
            }
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                // Mirror of the walk's await-origin arm.
                if let Some(origin) = self.ast.yield_into_await_origins.get(&eid).copied() {
                    let base = self.container_key_lookup(origin, scope)?;
                    return Some(self.keys.field(base, "value"));
                }
                let base = self.container_key_lookup(*obj, scope)?;
                Some(self.keys.field(base, name))
            }
            Expr::OptIndex { obj, .. } | Expr::Index { obj, .. } => {
                let base = self.container_key_lookup(*obj, scope)?;
                Some(self.keys.elem(base))
            }
            Expr::Call { callee, args } => {
                if let Some(f) = self.direct_callee(eid, *callee)
                    && self.fn_params.contains_key(&f)
                {
                    return Some(self.keys.ret(&f));
                }
                if self.retargets.contains_key(&eid)
                    || matches!(self.ast.get_expr(*callee), Expr::Ident(_))
                {
                    // Fn-valued binding (a parameter or local may
                    // shadow a declaration's name) or unknown global —
                    // the indirect projection (mirror of the walk side).
                    return self
                        .container_key_lookup(*callee, scope)
                        .map(|k| self.keys.field(k, "__ret"));
                }
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee) {
                    // ②.6b — mirror of the walk's promise_static_key.
                    if let Expr::Ident(ns) = self.ast.get_expr(*obj)
                        && ns == "Promise"
                        && self.resolve(ns, scope).is_none()
                    {
                        return Some(self.keys.anon(eid.0));
                    }
                    // Mirror of the walk's `Array.from` arm. Both static
                    // calls have an untrackable receiver — a global
                    // namespace ident — so the receiver lookup below
                    // fails and the whole call used to answer "no key"
                    // on this side while the walk keyed it by its Anon
                    // origin.
                    if let Expr::Ident(ns) = self.ast.get_expr(*obj)
                        && ns == "Array"
                        && self.resolve(ns, scope).is_none()
                    {
                        return (name == "from").then(|| self.keys.anon(eid.0));
                    }
                    let recv = self.container_key_lookup(*obj, scope)?;
                    return self.method_result_key_pure(eid, recv, name, args);
                }
                // F1 mirror — indirect call through a non-ident fn
                // value (`fs[0]()`): same `__ret` projection as the
                // walk side.
                self.container_key_lookup(*callee, scope)
                    .map(|k| self.keys.field(k, "__ret"))
            }
            Expr::Array(_)
            | Expr::ObjectLit { .. }
            | Expr::Ternary { .. }
            | Expr::Nullish { .. }
            | Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                ..
            } => Some(self.keys.anon(eid.0)),
            Expr::Sequence { right, .. } => self.container_key_lookup(*right, scope),
            Expr::As { expr, .. } => self.container_key_lookup(*expr, scope),
            Expr::Assign { value, .. } => self.container_key_lookup(*value, scope),
            _ => None,
        }
    }

    /// Pure key part of `method_result_key`: the same key for every
    /// name, none of the unions the walk attaches around it.
    ///
    /// The two lists are **hand-maintained mirrors, not one shared
    /// list** — the earlier claim here that they "can never disagree"
    /// was aspiration, and three names had already drifted apart
    /// (`toSpliced`, `valueOf`, and the `Array.from` prologue above).
    /// Nothing announces a drift: a name the walk knows but this side
    /// does not falls through to the `_` arm and answers the
    /// struct-field-fn projection `Field(Field(recv, name), "__ret")`,
    /// a key nobody populated — and an empty class defaults narrow, so
    /// f64 bits come back read as integers. The bound form stays right
    /// (the walk keyed it), which is why only the unbound reads —
    /// `take(xs.toSpliced(1, 1)[0])` — showed it.
    ///
    /// **Adding a name to `method_result_key` means adding it here.**
    fn method_result_key_pure(
        &self,
        eid: ExprId,
        recv: SlotKeyId,
        name: &str,
        args: &[ExprId],
    ) -> Option<SlotKeyId> {
        match name {
            "slice" | "filter" | "reverse" | "sort" | "toReversed" | "toSorted" | "splice"
            | "toSpliced" | "with" | "concat" | "flat" | "map" | "flatMap" | "fill"
            | "copyWithin" => Some(self.keys.anon(eid.0)),
            "pop" | "shift" | "at" | "find" | "findLast" => Some(self.keys.elem(recv)),
            // §20.1.3.7 — `Object.prototype.valueOf` answers `this`, so
            // the product IS the receiver. Same gate and spelling as the
            // walk's arm.
            "valueOf"
                if !self.any_class_owns_method("valueOf") || self.demoted.contains_key(&eid) =>
            {
                Some(recv)
            }
            // Map value slot (b1) — same gate + spelling as the walk.
            // Demoted call sites carry typed receiver evidence (check
            // proved a builtin container), so the class-name gate
            // doesn't apply to them.
            "get" if !self.any_class_owns_method("get") || self.demoted.contains_key(&eid) => {
                Some(self.keys.elem(recv))
            }
            "reduce" | "reduceRight" => args
                .first()
                .and_then(|a| self.callee_fn_name(*a))
                .map(|f| self.keys.ret(&f)),
            // ②.6b — same Anon spelling as the walk's promise arm.
            "then" | "catch" | "finally" => Some(self.keys.anon(eid.0)),
            _ => {
                let any = self
                    .classes
                    .iter()
                    .any(|c| self.fn_params.contains_key(&format!("__cm_{c}__{name}")));
                if any {
                    Some(self.keys.anon(eid.0))
                } else {
                    // F5 — struct-field-fn call: same `__ret`
                    // projection spelling as the walk side.
                    let fk = self.keys.field(recv, name);
                    Some(self.keys.field(fk, "__ret"))
                }
            }
        }
    }
}
