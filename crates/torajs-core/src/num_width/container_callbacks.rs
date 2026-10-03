//! Callback parameter wiring for the container methods that call
//! back: the element class flowing into the callback's element
//! parameters (map family, forEach, sort, reduce) and the receiver
//! joining the parameter that receives O. Split from
//! `container_methods.rs` (file-size limit).

use super::{Analysis, Scope, SlotKey};
use crate::ast::ExprId;

impl Analysis<'_> {
    /// An element class flowing into a callback's first user parameter:
    /// a one-way width edge always, plus the nested alias edge that
    /// only fires when the parameter is itself used as a container
    /// (`grid.map(row => row[0] = …)`); a scalar element parameter
    /// keeps the width edge alone.
    ///
    /// F5 — positional wiring must index the USER params: a lifted
    /// closure's raw param list starts with `__env`, so `ps.first()`
    /// wired the elem edge onto the env pointer (and reduce's acc
    /// feedback missed the accumulator entirely — the array-005 FPR
    /// abort).
    pub(super) fn wire_elem_to_cb_param(&mut self, ek: &SlotKey, cb_arg: ExprId) {
        self.wire_elem_to_cb_param_at(ek, cb_arg, 0);
    }

    /// The same wiring for a callback parameter other than the first.
    ///
    /// §24.2.3.6 hands a Set member as BOTH the value and the key, so
    /// both of a Set `forEach` callback's first two parameters see the
    /// receiver's elements. Without the second one it took the default
    /// I64 while the first was F64 — and an I64 slot cannot spell the
    /// undefined sentinel at all, so `s.forEach((v, v2) => typeof v2)`
    /// answered "number" for a member `typeof v` reported `undefined`.
    /// An Array index and a Map key are not elements, which is why
    /// this is not folded into the arm above.
    fn wire_elem_to_cb_param_at(&mut self, ek: &SlotKey, cb_arg: ExprId, idx: usize) {
        if let Some(cb) = self.callee_fn_name(cb_arg)
            && let Some(p) = self.callback_params(&cb).get(idx).cloned()
        {
            let pk = SlotKey::Param(cb, p);
            self.c_edges
                .entry(ek.clone())
                .or_default()
                .push((pk.clone(), false));
            self.nested_unions.push((ek.clone(), pk));
        }
    }

    /// `forEach`'s element wiring, which is per-receiver: a Set hands
    /// the member twice (see [`Self::wire_elem_to_cb_param_at`]).
    fn wire_foreach_params(&mut self, ek: &SlotKey, cb_arg: ExprId, recv: ExprId) {
        self.wire_elem_to_cb_param_at(ek, cb_arg, 0);
        if matches!(self.expr_types.get(&recv), Some(crate::check::Type::Set)) {
            self.wire_elem_to_cb_param_at(ek, cb_arg, 1);
        }
    }

    /// The callback edges of one member call `recv.name(args)`; `ek` is
    /// the receiver's element class.
    pub(super) fn callback_param_wiring(
        &mut self,
        name: &str,
        args: &[ExprId],
        obj: ExprId,
        ek: &SlotKey,
        recv: &SlotKey,
        scope: &Scope,
    ) {
        // Callback-taking iteration — the element param sees the
        // receiver's elems (value flow + nested-reference alias).
        match name {
            "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "some" | "every"
            | "flatMap" => {
                if let Some(a0) = args.first() {
                    if name == "forEach" {
                        self.wire_foreach_params(ek, *a0, obj);
                    } else {
                        self.wire_elem_to_cb_param(ek, *a0);
                    }
                }
            }
            "sort" | "toSorted" => {
                // Perf Round 5 F64-cmp fix (RFC 20260703) — BOTH
                // comparator params see the receiver's elems. Pre-fix
                // sort was absent from this match, so an F64-elem
                // array's comparator monoed to I64 params while the
                // sort call site passed f64 values — the callee then
                // read garbage from the integer registers (probe:
                // cmp(3.5, -1.25) received 4307779648 / 16384).
                if let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a)) {
                    for p in self.callback_params(&cb).iter().take(2).cloned() {
                        let pk = SlotKey::Param(cb.clone(), p);
                        self.c_edges
                            .entry(ek.clone())
                            .or_default()
                            .push((pk.clone(), false));
                        self.nested_unions.push((ek.clone(), pk));
                    }
                }
            }
            "reduce" | "reduceRight" => {
                if let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a)) {
                    let ps = self.callback_params(&cb);
                    if let Some(p0) = ps.first() {
                        // Accumulator: seeded by the init arg, fed back
                        // from the callback's own ret.
                        let pk = SlotKey::Param(cb.clone(), p0.clone());
                        if let Some(init) = args.get(1).copied() {
                            let w = self.width_of(init, scope);
                            self.add_container_constraint(pk.clone(), w);
                            self.alias_guarded(pk.clone(), init, scope);
                        }
                        self.c_edges
                            .entry(SlotKey::Ret(cb.clone()))
                            .or_default()
                            .push((pk, false));
                    }
                    if let Some(p1) = ps.get(1) {
                        let pk = SlotKey::Param(cb.clone(), p1.clone());
                        self.c_edges
                            .entry(ek.clone())
                            .or_default()
                            .push((pk.clone(), false));
                        self.guarded_unions.push((pk, ek.clone()));
                    }
                }
            }
            _ => {}
        }
        // §23.1.3's callback methods hand the receiver itself to the
        // callback as O (forEach: `Call(callbackfn, T, « kValue, k, O »)`),
        // so that parameter is the receiver's class: a width change
        // through it (`o[1] = 0.5`, `delete o[2]`) is a change to the
        // receiver. Unjoined, the receiver kept its I64 lane and the
        // write through O threw a kind TypeError.
        if let Some(o_idx) = callback_o_param_index(name)
            && let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a))
            && let Some(p) = self.callback_params(&cb).get(o_idx).cloned()
        {
            self.guarded_unions
                .push((SlotKey::Param(cb, p), recv.clone()));
        }
    }
}

/// The index of O among the user parameters of a callback method's
/// callback, for the methods that pass the receiver.
fn callback_o_param_index(method: &str) -> Option<usize> {
    match method {
        "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "findLastIndex"
        | "some" | "every" | "flatMap" => Some(2),
        "reduce" | "reduceRight" => Some(3),
        _ => None,
    }
}
