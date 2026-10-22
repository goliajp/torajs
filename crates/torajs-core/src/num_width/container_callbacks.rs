//! Callback parameter wiring for the container methods that call
//! back: the element class flowing into the callback's element
//! parameters (map family, forEach, sort, reduce) and the receiver
//! joining the parameter that receives O. Split from
//! `container_methods.rs` (file-size limit).

use super::{Analysis, Scope, SlotKeyId};
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
    ///
    /// An object element is the exception: the parameter then holds the
    /// element itself, so it IS the element's class and joins it
    /// outright. A width edge alone left `xs.map((m) => m.right)` reading
    /// a widened `{ right: number }` element through the parameter's
    /// own narrow layout, its fraction coming back as raw bits.
    pub(super) fn wire_elem_to_cb_param(
        &mut self,
        ek: SlotKeyId,
        src: ExprId,
        cb_arg: ExprId,
        scope: &Scope,
    ) {
        self.wire_elem_to_cb_param_at(ek, src, cb_arg, 0, scope);
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
    fn wire_elem_to_cb_param_at(
        &mut self,
        ek: SlotKeyId,
        src: ExprId,
        cb_arg: ExprId,
        idx: usize,
        scope: &Scope,
    ) {
        if let Some(pk) = self.cb_param_key(cb_arg, idx, scope) {
            self.c_edges.entry(ek).or_default().push((pk, false));
            if self.elems_are_objects(src) {
                self.guarded_unions.push((pk, ek));
            }
            self.nested_unions.push((ek, pk));
        }
    }

    fn elems_are_objects(&self, src: ExprId) -> bool {
        use crate::check::Type;
        matches!(self.expr_types.get(&src),
            Some(Type::Array(t)) if matches!(**t, Type::Struct(_) | Type::ClassRef(_)))
    }

    /// The key of a callback's `idx`-th parameter: the named function's
    /// own parameter, or, for a callback read out of a slot
    /// (`a.map(o.cb)`), that slot's `__p{idx}` projection, which
    /// [`Self::fn_value_flow`] joined with every function stored there.
    /// Without the projection the stored function's parameter kept the
    /// integer default and a 0.5 element reached it as 0.
    fn cb_param_key(&mut self, cb_arg: ExprId, idx: usize, scope: &Scope) -> Option<SlotKeyId> {
        match self.callee_fn_name(cb_arg) {
            Some(cb) => {
                let p = self.callback_params(&cb).get(idx).cloned()?;
                Some(self.keys.param(&cb, &p))
            }
            None => self.callback_projection(cb_arg, &format!("__p{idx}"), scope),
        }
    }

    /// The key of a callback's return point, by the same two spellings
    /// as [`Self::cb_param_key`].
    fn cb_ret_key(&mut self, cb_arg: ExprId, scope: &Scope) -> Option<SlotKeyId> {
        match self.callee_fn_name(cb_arg) {
            Some(cb) => Some(self.keys.ret(&cb)),
            None => self.callback_projection(cb_arg, "__ret", scope),
        }
    }

    /// `forEach`'s element wiring, which is per-receiver: a Set hands
    /// the member twice (see [`Self::wire_elem_to_cb_param_at`]).
    fn wire_foreach_params(&mut self, ek: SlotKeyId, cb_arg: ExprId, recv: ExprId, scope: &Scope) {
        self.wire_elem_to_cb_param_at(ek, recv, cb_arg, 0, scope);
        if matches!(self.expr_types.get(&recv), Some(crate::check::Type::Set)) {
            self.wire_elem_to_cb_param_at(ek, recv, cb_arg, 1, scope);
        }
    }

    /// The callback edges of one member call `recv.name(args)`; `ek` is
    /// the receiver's element class.
    pub(super) fn callback_param_wiring(
        &mut self,
        name: &str,
        args: &[ExprId],
        obj: ExprId,
        ek: SlotKeyId,
        recv: SlotKeyId,
        scope: &Scope,
    ) {
        // Callback-taking iteration — the element param sees the
        // receiver's elems (value flow + nested-reference alias).
        match name {
            "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "some" | "every"
            | "flatMap" => {
                if let Some(a0) = args.first() {
                    if name == "forEach" {
                        self.wire_foreach_params(ek, *a0, obj, scope);
                    } else {
                        self.wire_elem_to_cb_param(ek, obj, *a0, scope);
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
                if let Some(&cb_arg) = args.first() {
                    for idx in 0..2 {
                        self.wire_elem_to_cb_param_at(ek, obj, cb_arg, idx, scope);
                    }
                }
            }
            "reduce" | "reduceRight" => {
                if let Some(&cb_arg) = args.first() {
                    if let Some(pk) = self.cb_param_key(cb_arg, 0, scope) {
                        // Accumulator: seeded by the init arg, fed back
                        // from the callback's own ret.
                        if let Some(init) = args.get(1).copied() {
                            let w = self.flow(init, scope);
                            self.add_container_constraint(pk, w);
                            self.alias_guarded(pk, init, scope);
                        }
                        if let Some(rk) = self.cb_ret_key(cb_arg, scope) {
                            self.c_edges.entry(rk).or_default().push((pk, false));
                        }
                    }
                    if let Some(pk) = self.cb_param_key(cb_arg, 1, scope) {
                        self.c_edges.entry(ek).or_default().push((pk, false));
                        self.guarded_unions.push((pk, ek));
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
            && let Some(&cb_arg) = args.first()
            && let Some(pk) = self.cb_param_key(cb_arg, o_idx, scope)
        {
            self.guarded_unions.push((pk, recv));
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
