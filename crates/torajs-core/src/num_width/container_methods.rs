//! W4 / F5 — member-call width wiring: result-key resolution for
//! method calls and the side-effect edges every call site contributes
//! (element writes, callback parameter wiring, class-method
//! broadcast, struct-field-fn arg projections). Split from
//! `container_walk.rs` (file-size limit); the read-only mirror lives
//! in `container_lookup.rs`.

use super::{Analysis, Scope, SlotKey};
use crate::ast::PropKey;
use crate::ast::{Expr, ExprId};

mod arg_params;

/// Member methods with dedicated wiring in this file — the F5
/// struct-field-fn arg edges must not double-wire them.
const BUILTIN_MEMBER_METHODS: &[&str] = &[
    "slice",
    "filter",
    "reverse",
    "sort",
    "toReversed",
    "toSorted",
    "splice",
    "toSpliced",
    "with",
    "concat",
    "flat",
    "pop",
    "shift",
    "at",
    "find",
    "findLast",
    "map",
    "flatMap",
    "reduce",
    "reduceRight",
    "push",
    "unshift",
    "fill",
    "copyWithin",
    "forEach",
    "findIndex",
    "some",
    "every",
    "then",
    "catch",
    "finally",
    "get",
    "set",
    "add",
];

impl<'a> Analysis<'a> {
    /// Number-domain gate for a fn's first user param and ret: `true`
    /// when the annotation is in the number domain or absent
    /// (inference may go either way — union is the conservative,
    /// ABI-safe side for the numeric case and harmless for non-numeric
    /// slots, which no width query ever reads).
    ///
    /// RFC 20260726-array-elem-width — an array OF numbers is in that
    /// domain too. The gate used to admit the bare spelling only, so
    /// `Promise.all(ps).then((arr: number[]) => …)` left the handler's
    /// parameter unjoined from the value the promise settles with, and
    /// the two disagreed on the elements' width: the settled array
    /// holds integers while a widened parameter reads them as f64.
    /// `await` was never affected — it reads the value slot directly
    /// rather than through a handler.
    pub(super) fn fn_number_faces(&self, fname: &str) -> (bool, bool) {
        for stmt in &self.ast.stmts {
            if let crate::ast::Stmt::FnDecl {
                name,
                params,
                return_type,
                ..
            } = stmt
                && name == fname
            {
                let user: &[crate::ast::Param] =
                    if params.first().is_some_and(|p| p.name == "__env") {
                        &params[1..]
                    } else {
                        &params[..]
                    };
                let p = user.first().is_none_or(|p0| in_number_domain(&p0.type_ann));
                let r = in_number_domain(return_type);
                return (p, r);
            }
        }
        (false, false)
    }

    /// True when any user class declares a method `name` — those
    /// call sites must keep the `_` arm's every-owner Ret join
    /// instead of a builtin-container key spelling.
    pub(super) fn any_class_owns_method(&self, name: &str) -> bool {
        self.classes
            .iter()
            .any(|c| self.fn_params.contains_key(&format!("__cm_{c}__{name}")))
    }

    /// RFC 20260726-array-elem-width knife 8 — whether a callback
    /// answers an array. `flatMap` treats a non-array answer as the
    /// element itself (ES §23.1.3.11 step 8.d), which is a different
    /// element point from the one an array answer contributes.
    /// An unknown type keeps the array reading, the one that was
    /// unconditional before.
    pub(super) fn cb_returns_array(&self, cb: ExprId) -> bool {
        match self.expr_types.get(&cb) {
            Some(crate::check::Type::Function(_, ret)) => {
                matches!(**ret, crate::check::Type::Array(_))
            }
            _ => true,
        }
    }

    /// RFC 20260726-array-elem-width knife 10 — `Array.from(src, cb)`
    /// hands `cb` the source's elements exactly as the map family does,
    /// so its parameter has to see that element class.
    ///
    /// The namespace receiver is untrackable, so this call never
    /// reached the map family's callback wiring and the parameter kept
    /// whatever width its own annotation defaulted to. A fractional
    /// source then passed f64 elements into an i64 parameter and
    /// register allocation aborted on it — the same shape the sort
    /// comparator arm was fixed for.
    pub(super) fn array_from_wiring(&mut self, args: &[ExprId], scope: &Scope) {
        if let Some(a0) = args.first()
            && let Some(cb_arg) = args.get(1).copied()
            && let Some(sk) = self.container_key_of(*a0, scope)
        {
            self.wire_elem_to_cb_param(&SlotKey::Elem(Box::new(sk)), *a0, cb_arg, scope);
        }
    }

    /// Side effects of a member call — element writes (push family),
    /// callback parameter wiring, and user-class-method broadcast.
    /// Fires from walk_expr for every call site, result used or not.
    pub(super) fn member_call_effects(
        &mut self,
        eid: ExprId,
        callee: ExprId,
        args: &[ExprId],
        scope: &Scope,
    ) {
        let Expr::Member { obj, name } = self.ast.get_expr(callee).clone() else {
            return;
        };
        if self.namespace_call_effects(eid, obj, &name, args, scope) {
            return;
        }
        let Some(recv) = self.container_key_of(obj, scope) else {
            return;
        };
        if matches!(name.as_str(), "then" | "catch" | "finally") {
            self.promise_chain_wiring(eid, &recv, &name, args, scope);
        }
        self.mark_containerish(&recv);
        let ek = SlotKey::Elem(Box::new(recv.clone()));
        let write_args: Vec<ExprId> = match name.as_str() {
            "push" | "unshift" => args.to_vec(),
            "fill" => args.iter().take(1).copied().collect(),
            // toSpliced's items are Elem writes on the product, whose
            // class unions with the receiver's (result-key B1b) — so
            // feeding `ek` covers both. Missing pre-fix: a promoted
            // (W-ESC any-demoted) receiver's product materializes at
            // a generic call boundary by the class width, and the
            // un-fed 0.5 item truncated to 0 (L3b arg-pack entry).
            "splice" | "toSpliced" => args.iter().skip(2).copied().collect(),
            "with" => args.iter().skip(1).take(1).copied().collect(),
            // Map value slot (b1) — `m.set(k, v)` writes v into the
            // receiver's Elem class (read back by `get`'s result
            // key); `s.add(v)` is Set's single-value form. On a
            // non-container receiver the class has no consumer —
            // harmless, same bar as push on a user object.
            "set" => args.iter().skip(1).take(1).copied().collect(),
            "add" => args.iter().take(1).copied().collect(),
            _ => Vec::new(),
        };
        for a in write_args {
            // an `any` value appended to an annotated array is cast into
            // its element type, maybe rebuilt (`any_object_seed`)
            if matches!(name.as_str(), "push" | "unshift")
                && self.expr_types.get(&a) == Some(&crate::check::Type::Any)
            {
                for ann in self.pushed_elem_anns(obj, scope) {
                    self.any_object_seed(a, &ek, &ann);
                }
            }
            let w = self.flow(a, scope);
            self.add_container_constraint(ek.clone(), w);
            // A function value written into the element slot wires the
            // slot's signature projections onto its own (F5; 638-13).
            self.fn_value_flow(&ek, a, scope);
            // F2-fix — nested-reference alias only when the written
            // value is itself a container (candidate-side evidence);
            // scalar args contribute the one-way width edge above.
            if let Some(ak) = self.container_key_of(a, scope) {
                self.nested_unions.push((ek.clone(), ak));
            }
        }
        // §23.1.3.1 — `concat` copies its container arguments' slot
        // bits into a product of the receiver's class, and pushes its
        // scalar arguments into it. The result-key arm joins those
        // classes too, but only when something asks for the result's
        // key: `console.log(a.concat([9]))` never does, so `[9]` kept
        // its own narrow class and its i64 bits were read back as f64
        // (9 printed 4.4e-323). Every call site joins them here.
        if name == "concat" {
            for &a in args {
                let w = self.flow(a, scope);
                self.add_container_constraint(ek.clone(), w);
                if let Some(ak) = self.container_key_of(a, scope) {
                    self.mark_containerish(&ak);
                    self.uf.union(&recv, &ak);
                }
            }
        }
        if answers_undef_sentinel(&name) {
            self.add_container_constraint(ek.clone(), super::magnitude::f64_flow());
        }
        self.callback_param_wiring(&name, args, obj, &ek, &recv, scope);
        // User class method broadcast: receiver joins every owner
        // class; args flow into every owner's params (conservative —
        // static dispatch needs types the analysis doesn't have).
        let mut class_hit = false;
        for c in self.classes.clone() {
            let f = format!("__cm_{c}__{name}");
            if let Some(ps) = self.method_arg_params(&f) {
                class_hit = true;
                let ck = SlotKey::Class(c.clone());
                self.mark_containerish(&ck);
                self.uf.union(&recv, &ck);
                for (i, a) in args.iter().enumerate() {
                    if let Some(p) = ps.get(i + 1) {
                        let pk = SlotKey::Param(f.clone(), p.clone());
                        let w = self.flow(*a, scope);
                        self.add_container_constraint(pk.clone(), w);
                        self.alias_guarded(pk, *a, scope);
                    }
                }
            }
        }
        // F5 — struct-field-fn call: args flow into the field value's
        // `__p{i}` projections (the member-callee mirror of the F1
        // indirect Param edges). Builtin container methods keep their
        // dedicated wiring above; projections nobody populated are
        // dead keys.
        if !class_hit && !BUILTIN_MEMBER_METHODS.contains(&name.as_str()) {
            let fk = SlotKey::Field(Box::new(recv.clone()), PropKey::from(name));
            self.record_variadic_call(callee, args, &fk);
            for (i, a) in args.iter().enumerate() {
                let w = self.flow(*a, scope);
                let pk = SlotKey::Field(Box::new(fk.clone()), PropKey::from(format!("__p{i}")));
                self.add_constraint(pk.clone(), w);
                // RFC 20260726-array-elem-width, member-callee half — a
                // CONTAINER arg JOINS the projection, it does not merely
                // hand over its scalar width (direct + F1 arms both do
                // this). Without it `ns.take(xs)` read its array from a
                // different element class than the caller filled: an
                // `any`-held block met a raw f64 loader and answered NaN.
                self.alias_guarded(pk.clone(), *a, scope);
                self.fn_value_flow(&pk, *a, scope);
            }
        }
    }
}

/// True for the methods that can answer `undefined` about an element,
/// which the F64 undefined-NaN sentinel is the only numeric repr of:
/// an I64-narrowed element slot cannot hold it, so being called is
/// itself a reason to widen the receiver's element class. Non-numeric
/// element classes never consume width seeds — no repr change there.
///
/// - RFC 20260722-find-miss chunk D — `find` / `findLast` miss.
/// - `at` out of range (§23.1.3.1 step 6). It lowers through the same
///   checked index branch as `xs[i]`, it just isn't spelled that way,
///   so the index-read seed doesn't see it.
/// - `pop` / `shift` on an EMPTY array (§23.1.3.22 step 3.a,
///   §23.1.3.25 step 3.a) — a different exit from the out-of-range
///   one, and the last numeric shape that had nowhere to put the
///   answer: the all-integral form answered the slot's zero where the
///   same array holding one fraction already answered `undefined`.
/// - `get` on a `Map<K, number>` miss (§24.1.3.6 step 5) — the typed
///   value lane (`ssa_lower_map_num_lane`) reads the value as an f64.
fn answers_undef_sentinel(name: &str) -> bool {
    matches!(name, "find" | "findLast" | "at" | "pop" | "shift" | "get")
}

/// True when an annotation names a number or a nesting of arrays of
/// them (`number`, `number[]`, `number[][]`), or is absent.
///
/// Only the elements of such a container ever carry a width, so joining
/// on the container is what carries the elements along.
pub(super) fn in_number_domain(ann: &Option<String>) -> bool {
    match ann.as_deref() {
        None => true,
        Some(t) => t.trim_end_matches("[]") == "number",
    }
}
