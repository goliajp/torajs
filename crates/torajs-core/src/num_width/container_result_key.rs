//! W4 / F5 — member-call result-key resolution
//! ([`Analysis::method_result_key`]) — split from
//! `container_methods.rs` (file-size limit; RFC
//! 20260713-array-proto-residual B1b grew the transform arm past
//! 500). Side-effect wiring (`member_call_effects`) stays in the
//! parent file.

use super::{Analysis, Scope, SlotKey};
use crate::ast::Expr;
use crate::ast::ExprId;

impl<'a> Analysis<'a> {
    /// Result container key of a member call, for flows consuming it.
    /// Transform plumbing (slice / concat / map …) is unconditional —
    /// nested element references alias through the result.
    pub(super) fn method_result_key(
        &mut self,
        eid: ExprId,
        obj: ExprId,
        name: &str,
        args: &[ExprId],
        scope: &Scope,
    ) -> Option<SlotKey> {
        // ②.6b — `Promise.<static>(..)` has an untrackable receiver
        // (the global namespace ident); key the result by its Anon
        // origin (wiring fires from member_call_effects).
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Promise"
            && self.resolve(ns, scope).is_none()
        {
            let anon = SlotKey::Anon(eid.0);
            self.mark_containerish(&anon);
            return Some(anon);
        }
        // RFC 20260726-array-elem-width — `Array.from(src)` has the same
        // untrackable-receiver shape as the Promise statics: `Array` is
        // a global namespace ident, so the receiver lookup below fails
        // and the whole call used to answer None. With no key, nothing
        // joined the product to the binding that holds it — the binding
        // widened on a fractional write while the copied source stayed
        // narrow, and the reads reinterpreted the bits.
        //
        // The product memcpys the source's slots (the 1-arg path is an
        // `arr_slice` clone), so the two share one repr class exactly
        // like `slice` / `concat` below.
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Array"
            && self.resolve(ns, scope).is_none()
        {
            if name != "from" {
                return None;
            }
            let anon = SlotKey::Anon(eid.0);
            self.mark_containerish(&anon);
            if let Some(ak) = args.first().and_then(|a| self.container_key_of(*a, scope)) {
                self.uf.union(
                    &SlotKey::Elem(Box::new(anon.clone())),
                    &SlotKey::Elem(Box::new(ak.clone())),
                );
                self.uf.union(&anon, &ak);
            }
            return Some(anon);
        }
        let recv = self.container_key_of(obj, scope)?;
        self.mark_containerish(&recv);
        let ek = SlotKey::Elem(Box::new(recv.clone()));
        match name {
            "slice" | "filter" | "reverse" | "sort" | "toReversed" | "toSorted" | "splice"
            | "toSpliced" | "with" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                self.uf.union(&SlotKey::Elem(Box::new(anon.clone())), &ek);
                // RFC 20260713-array-proto-residual B1b — the product
                // memcpys the receiver's slot bits verbatim (reverse /
                // sort even return the same pointer), so the two
                // containers share one repr class: a W-ESC any-escape
                // face on either side must demote both, or the
                // escaped side stores NaN-boxes that the typed side
                // reads raw (appeared-4 sweep regression).
                self.uf.union(&anon, &recv);
                Some(anon)
            }
            "concat" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                let aek = SlotKey::Elem(Box::new(anon.clone()));
                self.uf.union(&aek, &ek);
                // B1b — product bits memcpy from the receiver (and
                // container args below): one repr class each.
                self.uf.union(&anon, &recv);
                for a in args.to_vec() {
                    // F2-fix — `concat` accepts scalars too; alias the
                    // arg's element class only with candidate-side
                    // container evidence, and feed its scalar width
                    // either way.
                    let w = self.width_of(a, scope);
                    self.add_container_constraint(aek.clone(), w);
                    if let Some(ak) = self.container_key_of(a, scope) {
                        self.uf.union(&anon, &ak);
                        self.nested_unions
                            .push((aek.clone(), SlotKey::Elem(Box::new(ak))));
                    }
                }
                Some(anon)
            }
            "flat" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                self.uf.union(
                    &SlotKey::Elem(Box::new(anon.clone())),
                    &SlotKey::Elem(Box::new(SlotKey::Elem(Box::new(recv)))),
                );
                // B1b — flat's product bits memcpy from the INNER
                // arrays, whose container class is the receiver's
                // element class.
                self.uf.union(&anon, &ek);
                Some(anon)
            }
            "pop" | "shift" | "at" | "find" | "findLast" => Some(ek),
            // Map value slot (b1) — `m.get(k)` reads the same class
            // `m.set(k, v)` writes (the Elem of the receiver, same
            // shape as Array's at/pop). Gated on no user class owning
            // a `get` method: those must keep the `_` arm's
            // every-owner Ret join (dispatch-face negotiation) —
            // EXCEPT demoted call sites, whose receiver check proved
            // a builtin container (typed evidence beats name gate).
            "get" if !self.any_class_owns_method("get") || self.demoted.contains_key(&eid) => {
                Some(ek)
            }
            "map" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                let aek = SlotKey::Elem(Box::new(anon.clone()));
                match args.first().and_then(|a| self.callee_fn_name(*a)) {
                    Some(cb) => {
                        let rk = SlotKey::Ret(cb);
                        self.c_edges
                            .entry(rk.clone())
                            .or_default()
                            .push((aek.clone(), false));
                        self.guarded_unions.push((aek, rk));
                    }
                    // Opaque callback — element width unprovable.
                    None => self.c_seeds.push(aek),
                }
                Some(anon)
            }
            "flatMap" => Some(self.flat_map_result_key(eid, args)),
            "reduce" | "reduceRight" => args
                .first()
                .and_then(|a| self.callee_fn_name(*a))
                .map(SlotKey::Ret),
            // ②.6b — promise chain result keys by its Anon origin
            // (wiring fires from member_call_effects). The `value`
            // spelling matches the parser's `await p` → `p.value`
            // desugar, so await reads need no extra wiring.
            "then" | "catch" | "finally" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                Some(anon)
            }
            _ => {
                // User class method — the AST keeps `q.m()` as a Member
                // call; which `__cm_<C>__m` lowering picks needs types
                // the analysis doesn't have, so the result joins every
                // owner's ret through the Anon key.
                let mut any = false;
                let anon = SlotKey::Anon(eid.0);
                for c in self.classes.clone() {
                    let f = format!("__cm_{c}__{name}");
                    if self.fn_params.contains_key(&f) {
                        self.guarded_unions.push((anon.clone(), SlotKey::Ret(f)));
                        any = true;
                    }
                }
                if any {
                    Some(anon)
                } else {
                    // F5 — no class owns the method: a struct-field-fn
                    // call. The result reads the field value's `__ret`
                    // projection (glued onto the resident fn's Ret by
                    // the fill site's `fn_value_flow`); keys nobody
                    // populated stay narrow.
                    Some(SlotKey::Field(
                        Box::new(SlotKey::Field(Box::new(recv), name.to_string())),
                        "__ret".to_string(),
                    ))
                }
            }
        }
    }

    /// The product point of `xs.flatMap(cb)`, joined to whichever point
    /// of the callback supplies its elements.
    ///
    /// RFC 20260726-array-elem-width knife 8 — a callback answering a
    /// non-array acts like `[U]` per ES §23.1.3.11 step 8.d, so its
    /// return IS an element rather than a container holding one.
    /// Spelling that case the array way named the element of a scalar,
    /// a class nothing else is in, so the product never joined the
    /// binding that received it and the two disagreed on width.
    fn flat_map_result_key(&mut self, eid: ExprId, args: &[ExprId]) -> SlotKey {
        let anon = SlotKey::Anon(eid.0);
        self.mark_containerish(&anon);
        let aek = SlotKey::Elem(Box::new(anon.clone()));
        match args.first().and_then(|a| self.callee_fn_name(*a)) {
            Some(cb) => {
                let rk = SlotKey::Ret(cb);
                let elem_src = if self.cb_returns_array(args[0]) {
                    SlotKey::Elem(Box::new(rk))
                } else {
                    rk
                };
                self.uf.union(&aek, &elem_src);
            }
            None => self.c_seeds.push(aek),
        }
        anon
    }
}
