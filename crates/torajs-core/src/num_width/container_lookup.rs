//! W4 — read-only container-key resolution: the `&self` mirror of
//! `container_walk.rs`'s flow-site walkers, consumed by `width_of`
//! (which cannot take the side-effecting `&mut` path; the walk has
//! already attached Anon constraints and unions at flow sites).

use super::{Analysis, Scope, SlotKey};
use crate::ast::{Expr, ExprId};

impl<'a> Analysis<'a> {
    /// Read-only mirror of `container_key_of` — same key for every
    /// shape, none of the side effects (Anon constraints, unions).
    /// `width_of` consumes this: it runs under `&self` and the walk
    /// has already attached the side effects at flow sites.
    pub(super) fn container_key_lookup(&self, eid: ExprId, scope: &Scope) -> Option<SlotKey> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) => {
                if let Some(k) = self.resolve(n, scope) {
                    return Some(k);
                }
                if scope.fn_name.starts_with("__") && self.by_name.contains_key(n) {
                    return Some(SlotKey::Captured(n.clone()));
                }
                None
            }
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                let base = self.container_key_lookup(*obj, scope)?;
                Some(SlotKey::Field(Box::new(base), name.clone()))
            }
            Expr::OptIndex { obj, .. } | Expr::Index { obj, .. } => {
                let base = self.container_key_lookup(*obj, scope)?;
                Some(SlotKey::Elem(Box::new(base)))
            }
            Expr::Call { callee, args } => {
                if let Some(f) =
                    self.retargets
                        .get(&eid)
                        .cloned()
                        .or_else(|| match self.ast.get_expr(*callee) {
                            Expr::Ident(n) => Some(n.clone()),
                            _ => None,
                        })
                {
                    if self.fn_params.contains_key(&f) {
                        return Some(SlotKey::Ret(f));
                    }
                    return None;
                }
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee) {
                    // ②.6b — mirror of the walk's promise_static_key.
                    if let Expr::Ident(ns) = self.ast.get_expr(*obj)
                        && ns == "Promise"
                        && self.resolve(ns, scope).is_none()
                    {
                        return Some(SlotKey::Anon(eid.0));
                    }
                    let recv = self.container_key_lookup(*obj, scope)?;
                    return self.method_result_key_pure(eid, recv, name, args);
                }
                None
            }
            Expr::Array(_)
            | Expr::ObjectLit { .. }
            | Expr::Ternary { .. }
            | Expr::Nullish { .. } => Some(SlotKey::Anon(eid.0)),
            Expr::Sequence { right, .. } => self.container_key_lookup(*right, scope),
            Expr::As { expr, .. } => self.container_key_lookup(*expr, scope),
            Expr::Assign { value, .. } => self.container_key_lookup(*value, scope),
            _ => None,
        }
    }

    /// Pure key part of `method_result_key` — shared by the walk
    /// (which attaches unions around it) and the read-only lookup, so
    /// the two can never disagree on a key spelling.
    fn method_result_key_pure(
        &self,
        eid: ExprId,
        recv: SlotKey,
        name: &str,
        args: &[ExprId],
    ) -> Option<SlotKey> {
        match name {
            "slice" | "filter" | "reverse" | "sort" | "toReversed" | "toSorted" | "splice"
            | "with" | "concat" | "flat" | "map" | "flatMap" | "fill" | "copyWithin" => {
                Some(SlotKey::Anon(eid.0))
            }
            "pop" | "shift" | "at" | "find" | "findLast" => Some(SlotKey::Elem(Box::new(recv))),
            // Map value slot (b1) — same gate + spelling as the walk.
            // Demoted call sites carry typed receiver evidence (check
            // proved a builtin container), so the class-name gate
            // doesn't apply to them.
            "get" if !self.any_class_owns_method("get") || self.demoted.contains_key(&eid) => {
                Some(SlotKey::Elem(Box::new(recv)))
            }
            "reduce" | "reduceRight" => args
                .first()
                .and_then(|a| self.callee_fn_name(*a))
                .map(SlotKey::Ret),
            // ②.6b — same Anon spelling as the walk's promise arm.
            "then" | "catch" | "finally" => Some(SlotKey::Anon(eid.0)),
            _ => {
                let any = self
                    .classes
                    .iter()
                    .any(|c| self.fn_params.contains_key(&format!("__cm_{c}__{name}")));
                if any {
                    Some(SlotKey::Anon(eid.0))
                } else {
                    // F5 — struct-field-fn call: same `__ret`
                    // projection spelling as the walk side.
                    Some(SlotKey::Field(
                        Box::new(SlotKey::Field(Box::new(recv), name.to_string())),
                        "__ret".to_string(),
                    ))
                }
            }
        }
    }
}
