//! W4 / F5 — member-call width wiring: result-key resolution for
//! method calls and the side-effect edges every call site contributes
//! (element writes, callback parameter wiring, class-method
//! broadcast, struct-field-fn arg projections). Split from
//! `container_walk.rs` (file-size limit); the read-only mirror lives
//! in `container_lookup.rs`.

use super::{Analysis, Scope, SlotKey};
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
    "forEach",
    "findIndex",
    "some",
    "every",
];

impl<'a> Analysis<'a> {
    pub(super) fn callee_fn_name(&self, eid: ExprId) -> Option<String> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) if self.fn_params.contains_key(n) => Some(n.clone()),
            Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
            _ => None,
        }
    }

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
        let recv = self.container_key_of(obj, scope)?;
        self.mark_containerish(&recv);
        let ek = SlotKey::Elem(Box::new(recv.clone()));
        match name {
            "slice" | "filter" | "reverse" | "sort" | "toReversed" | "toSorted" | "splice"
            | "with" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                self.uf.union(&SlotKey::Elem(Box::new(anon.clone())), &ek);
                Some(anon)
            }
            "concat" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                let aek = SlotKey::Elem(Box::new(anon.clone()));
                self.uf.union(&aek, &ek);
                for a in args.to_vec() {
                    // F2-fix — `concat` accepts scalars too; alias the
                    // arg's element class only with candidate-side
                    // container evidence, and feed its scalar width
                    // either way.
                    let w = self.width_of(a, scope);
                    self.add_container_constraint(aek.clone(), w);
                    if let Some(ak) = self.container_key_of(a, scope) {
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
                Some(anon)
            }
            "pop" | "shift" | "at" | "find" | "findLast" => Some(ek),
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
            "flatMap" => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                let aek = SlotKey::Elem(Box::new(anon.clone()));
                match args.first().and_then(|a| self.callee_fn_name(*a)) {
                    Some(cb) => {
                        self.uf
                            .union(&aek, &SlotKey::Elem(Box::new(SlotKey::Ret(cb))));
                    }
                    None => self.c_seeds.push(aek),
                }
                Some(anon)
            }
            "reduce" | "reduceRight" => args
                .first()
                .and_then(|a| self.callee_fn_name(*a))
                .map(SlotKey::Ret),
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

    /// Side effects of a member call — element writes (push family),
    /// callback parameter wiring, and user-class-method broadcast.
    /// Fires from walk_expr for every call site, result used or not.
    pub(super) fn member_call_effects(&mut self, callee: ExprId, args: &[ExprId], scope: &Scope) {
        let Expr::Member { obj, name } = self.ast.get_expr(callee).clone() else {
            return;
        };
        let Some(recv) = self.container_key_of(obj, scope) else {
            return;
        };
        self.mark_containerish(&recv);
        let ek = SlotKey::Elem(Box::new(recv.clone()));
        let write_args: Vec<ExprId> = match name.as_str() {
            "push" | "unshift" => args.to_vec(),
            "fill" => args.iter().take(1).copied().collect(),
            "splice" => args.iter().skip(2).copied().collect(),
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
        // Callback-taking iteration — the element param sees the
        // receiver's elems (value flow + nested-reference alias).
        match name.as_str() {
            "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "some" | "every"
            | "flatMap" => {
                // F5 — positional wiring must index the USER params: a
                // lifted closure's raw param list starts with `__env`,
                // so `ps.first()` wired the elem edge onto the env
                // pointer (and reduce's acc feedback below missed the
                // accumulator entirely — the array-005 FPR abort).
                if let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a))
                    && let Some(p0) = self.user_params(&cb).first().cloned()
                {
                    let pk = SlotKey::Param(cb, p0);
                    self.c_edges
                        .entry(ek.clone())
                        .or_default()
                        .push((pk.clone(), false));
                    // F2-fix — the param aliases the element class
                    // only when it is used as a container itself
                    // (`grid.map(row => row[0] = …)`); a scalar elem
                    // param keeps the one-way width edge above.
                    self.nested_unions.push((ek.clone(), pk));
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
            let fk = SlotKey::Field(Box::new(recv.clone()), name.clone());
            for (i, a) in args.iter().enumerate() {
                let w = self.width_of(*a, scope);
                let pk = SlotKey::Field(Box::new(fk.clone()), format!("__p{i}"));
                self.add_constraint(pk.clone(), w);
                self.fn_value_flow(&pk, *a, scope);
            }
        }
    }
}
