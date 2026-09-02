//! W4 / F5 — member-call width wiring: result-key resolution for
//! method calls and the side-effect edges every call site contributes
//! (element writes, callback parameter wiring, class-method
//! broadcast, struct-field-fn arg projections). Split from
//! `container_walk.rs` (file-size limit); the read-only mirror lives
//! in `container_lookup.rs`.

use super::{Analysis, Scope, SlotKey};
use crate::ast::PropKey;
use crate::ast::{Expr, ExprId};

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
    /// ②.6b — value-point wiring for the Promise statics. The
    /// single-value statics (`resolve` / `reject`) feed the argument's
    /// width into the result's `value` point; thenable absorption
    /// passes the inner promise's point through. The combinators
    /// (`all` / `race` / `any`) fan the source elements' `value`
    /// points into the result's: `all` resolves to an ARRAY of those
    /// values (Elem of the result point), `race` / `any` to ONE of
    /// them (the result point itself). `allSettled` stays out — its
    /// value is an array of {status, value} wrappers, a struct face
    /// this table doesn't model yet. Fired from member_call_effects
    /// (every call site), keyed by the call's Anon origin.
    fn promise_static_wiring(&mut self, eid: ExprId, name: &str, args: &[ExprId], scope: &Scope) {
        let anon = SlotKey::Anon(eid.0);
        self.mark_containerish(&anon);
        let pv = SlotKey::Field(Box::new(anon.clone()), "value".into());
        if matches!(name, "resolve" | "reject")
            && let Some(a0) = args.first()
        {
            let w = self.width_of(*a0, scope);
            self.add_container_constraint(pv.clone(), w);
            if let Some(ak) = self.container_key_of(*a0, scope) {
                // RFC 20260726-array-elem-width — which point the
                // argument contributes depends on what it IS. Handing
                // `resolve` another promise passes that promise's value
                // through, so the two value points join. Handing it a
                // plain container makes that container the value, so
                // the argument's OWN point is what joins.
                //
                // Only the first reading existed, so an array literal
                // in `Promise.resolve([1, 2, 3])` sat in a different
                // class from the binding that awaited it: the binding
                // widened on a fractional write and read the untouched
                // i64 literal back as 1e-323, silently.
                if matches!(
                    self.expr_types.get(a0),
                    Some(crate::check::Type::Promise(_))
                ) {
                    self.uf
                        .union(&pv, &SlotKey::Field(Box::new(ak), "value".into()));
                } else {
                    self.uf.union(&pv, &ak);
                }
            }
        }
        if matches!(name, "all" | "race" | "any")
            && let Some(a0) = args.first()
            && let Some(ak) = self.container_key_of(*a0, scope)
        {
            let src_elem_pv = SlotKey::Field(Box::new(SlotKey::Elem(Box::new(ak))), "value".into());
            if name == "all" {
                // The result's value is a fresh array the runtime
                // fills from each source promise's raw value slot —
                // container evidence on the point itself, so the
                // `let r = await Promise.all(...)` guarded copy edge
                // activates.
                self.mark_containerish(&pv);
                self.uf
                    .union(&SlotKey::Elem(Box::new(pv.clone())), &src_elem_pv);
            } else {
                self.uf.union(&pv, &src_elem_pv);
            }
        }
    }

    /// ②.6b — value-point wiring for a promise chain call. The value
    /// slot is raw 8 bytes the runtime moves blindly (settled-state
    /// passthrough, cb invocation, resolve(result, cb_ret)), so every
    /// party touching one slot unions into one ABI class: source and
    /// result `value` points plus each handler's user param / ret.
    ///
    /// Each cb face unions ONLY when its annotation is number-domain
    /// (`: number` or unannotated) — promise slots carry any type,
    /// and gluing a `(s: string) => number` param into the numeric
    /// class poisons it (a T→U chain even closes a growth cycle
    /// through the passthrough union: Ret(tagN) ≡ Param(tagN) seeded
    /// the whole chain F64 — the async-025 regression). The width
    /// table only ever answers number-faced queries, so non-number
    /// faces simply stay out.
    fn promise_chain_wiring(&mut self, eid: ExprId, recv: &SlotKey, name: &str, args: &[ExprId]) {
        let anon = SlotKey::Anon(eid.0);
        self.mark_containerish(&anon);
        let src_pv = SlotKey::Field(Box::new(recv.clone()), "value".into());
        let res_pv = SlotKey::Field(Box::new(anon), "value".into());
        self.uf.union(&res_pv, &src_pv);
        if name != "finally" {
            // `.then(onOk, onErr)` — both handlers read the same
            // source slot and resolve the same result.
            for a in args.iter().take(2) {
                if let Some(cb) = self.callee_fn_name(*a) {
                    let (p_num, r_num) = self.fn_number_faces(&cb);
                    if p_num && let Some(p0) = self.user_params(&cb).first() {
                        self.uf
                            .union(&src_pv, &SlotKey::Param(cb.clone(), p0.clone()));
                    }
                    if r_num {
                        self.uf.union(&res_pv, &SlotKey::Ret(cb));
                    }
                }
            }
        }
    }

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
    fn fn_number_faces(&self, fname: &str) -> (bool, bool) {
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

    pub(super) fn callee_fn_name(&self, eid: ExprId) -> Option<String> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) if self.fn_params.contains_key(n) => Some(n.clone()),
            Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
            _ => None,
        }
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
    fn array_from_wiring(&mut self, args: &[ExprId], scope: &Scope) {
        if let Some(a0) = args.first()
            && let Some(cb_arg) = args.get(1).copied()
            && let Some(sk) = self.container_key_of(*a0, scope)
        {
            self.wire_elem_to_cb_param(&SlotKey::Elem(Box::new(sk)), cb_arg);
        }
    }

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
    fn wire_elem_to_cb_param(&mut self, ek: &SlotKey, cb_arg: ExprId) {
        if let Some(cb) = self.callee_fn_name(cb_arg)
            && let Some(p0) = self.user_params(&cb).first().cloned()
        {
            let pk = SlotKey::Param(cb, p0);
            self.c_edges
                .entry(ek.clone())
                .or_default()
                .push((pk.clone(), false));
            self.nested_unions.push((ek.clone(), pk));
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
        // ②.6b — Promise statics have an untrackable receiver (the
        // global namespace ident); their value-point wiring must fire
        // from here, the every-call-site hook (the result-key path
        // only fires when the result is consumed via a tracked flow —
        // `console.log(await Promise.resolve(2.5))` never is).
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Promise"
            && self.resolve(ns, scope).is_none()
        {
            self.promise_static_wiring(eid, &name, args, scope);
            return;
        }
        if let Expr::Ident(ns) = self.ast.get_expr(obj)
            && ns == "Array"
            && name == "from"
            && self.resolve(ns, scope).is_none()
        {
            self.array_from_wiring(args, scope);
            return;
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
            && matches!(name.as_str(), "defineProperty" | "defineProperties")
        {
            if let Some(a0) = args.first()
                && let Some(rk) = self.container_key_of(*a0, scope)
            {
                self.any_seeds.push(rk);
            }
            return;
        }
        let Some(recv) = self.container_key_of(obj, scope) else {
            return;
        };
        if matches!(name.as_str(), "then" | "catch" | "finally") {
            self.promise_chain_wiring(eid, &recv, &name, args);
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
            let w = self.width_of(a, scope);
            self.add_container_constraint(ek.clone(), w);
            // F2-fix — nested-reference alias only when the written
            // value is itself a container (candidate-side evidence);
            // scalar args contribute the one-way width edge above.
            if let Some(ak) = self.container_key_of(a, scope) {
                self.nested_unions.push((ek.clone(), ak));
            }
        }
        if answers_undef_sentinel(&name) {
            self.add_container_constraint(ek.clone(), super::W::F64);
        }
        // Callback-taking iteration — the element param sees the
        // receiver's elems (value flow + nested-reference alias).
        match name.as_str() {
            "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "some" | "every"
            | "flatMap" => {
                if let Some(a0) = args.first() {
                    self.wire_elem_to_cb_param(&ek, *a0);
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
                    for p in self.user_params(&cb).iter().take(2).cloned() {
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
                    let ps = self.user_params(&cb);
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
        // User class method broadcast: receiver joins every owner
        // class; args flow into every owner's params (conservative —
        // static dispatch needs types the analysis doesn't have).
        let mut class_hit = false;
        for c in self.classes.clone() {
            let f = format!("__cm_{c}__{name}");
            if let Some(ps) = self.fn_params.get(&f).cloned() {
                class_hit = true;
                let ck = SlotKey::Class(c.clone());
                self.mark_containerish(&ck);
                self.uf.union(&recv, &ck);
                for (i, a) in args.iter().enumerate() {
                    if let Some(p) = ps.get(i + 1) {
                        let pk = SlotKey::Param(f.clone(), p.clone());
                        let w = self.width_of(*a, scope);
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
            for (i, a) in args.iter().enumerate() {
                let w = self.width_of(*a, scope);
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
fn answers_undef_sentinel(name: &str) -> bool {
    matches!(name, "find" | "findLast" | "at" | "pop" | "shift")
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
