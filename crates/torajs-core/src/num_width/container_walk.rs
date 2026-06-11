//! W4 — constraint-walk hooks for the container face: container-key
//! resolution, member-call element effects, and element / field
//! assignment constraints. The union-find / activation / congruence
//! machinery these feed lives in `container.rs`.

use super::{Analysis, Scope, SlotKey, W};
use crate::ast::{Expr, ExprId};

impl<'a> Analysis<'a> {
    /// Container-channel constraint — same shape as `add_constraint`
    /// but lands in `c_seeds`/`c_edges`, which only the canonical
    /// fixpoint merges in. Keeps the scalar (pre-W4) fixpoint
    /// bit-identical until D2/D3 wire the consuming lowering sites.
    pub(super) fn add_container_constraint(&mut self, target: SlotKey, w: W) {
        match w {
            W::F64 => self.c_seeds.push(target),
            W::Num(deps) => {
                for (d, growth) in deps {
                    self.c_edges
                        .entry(d)
                        .or_default()
                        .push((target.clone(), growth));
                }
            }
            W::NotNum => {}
        }
    }

    /// Mark a key (and the receivers it nests in) as container
    /// evidence for the guarded-union activation.
    pub(super) fn mark_containerish(&mut self, k: &SlotKey) {
        if self.containerish.insert(k.clone()) {
            match k {
                SlotKey::Elem(x) | SlotKey::Field(x, _) => {
                    let inner = (**x).clone();
                    self.mark_containerish(&inner);
                }
                _ => {}
            }
        }
    }

    /// Guarded alias edge from a value-copy site.
    pub(super) fn alias_guarded(&mut self, dst: SlotKey, src_eid: ExprId, scope: &Scope) {
        if let Some(k) = self.container_key_of(src_eid, scope) {
            self.guarded_unions.push((dst, k));
        }
    }

    /// Container origin of an expression; None = not container-shaped
    /// (scalar literals, unknown intrinsics). Literal arms attach
    /// their element / field constraints on first construction —
    /// revisits re-push identical constraints, which the set-based
    /// fixpoint absorbs.
    pub(super) fn container_key_of(&mut self, eid: ExprId, scope: &Scope) -> Option<SlotKey> {
        let e = self.ast.get_expr(eid).clone();
        match &e {
            Expr::Ident(n) => {
                if let Some(k) = self.resolve(n, scope) {
                    return Some(k);
                }
                // Post-lift closure body: a captured name's defining
                // scope is gone — alias every same-named slot through
                // a shared key (conservative merge, width-only cost,
                // mirrors the scalar by_name broadcast).
                if scope.fn_name.starts_with("__")
                    && let Some(keys) = self.by_name.get(n).cloned()
                {
                    let ck = SlotKey::Captured(n.clone());
                    for k in &keys {
                        self.uf.union(&ck, k);
                    }
                    return Some(ck);
                }
                None
            }
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                let base = self.container_key_of(*obj, scope)?;
                Some(SlotKey::Field(Box::new(base), name.clone()))
            }
            Expr::Index { obj, .. } => {
                let base = self.container_key_of(*obj, scope)?;
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
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee).clone() {
                    return self.method_result_key(eid, obj, &name, args, scope);
                }
                None
            }
            Expr::Array(els) => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                let ek = SlotKey::Elem(Box::new(anon.clone()));
                for el in els.clone() {
                    if let Expr::Spread { expr } = self.ast.get_expr(el) {
                        let inner = *expr;
                        match self.container_key_of(inner, scope) {
                            Some(sk) => {
                                // Spread copies the source's slots —
                                // scalar widths flow forward; nested
                                // references alias, so the elem
                                // classes merge (conservative for the
                                // scalar case: width-only cost).
                                self.uf.union(&ek, &SlotKey::Elem(Box::new(sk)));
                            }
                            None => self.c_seeds.push(ek.clone()),
                        }
                        continue;
                    }
                    let w = self.width_of(el, scope);
                    self.add_container_constraint(ek.clone(), w);
                    if let Some(ik) = self.container_key_of(el, scope) {
                        // A container stored as an element is aliased
                        // by the slot — unconditional.
                        self.uf.union(&ek, &ik);
                    }
                }
                Some(anon)
            }
            Expr::ObjectLit { fields } => {
                let anon = SlotKey::Anon(eid.0);
                self.mark_containerish(&anon);
                for (fname, fe) in fields.clone() {
                    let fk = SlotKey::Field(Box::new(anon.clone()), fname.clone());
                    let w = self.width_of(fe, scope);
                    self.add_container_constraint(fk.clone(), w);
                    if let Some(ik) = self.container_key_of(fe, scope) {
                        self.uf.union(&fk, &ik);
                    }
                }
                Some(anon)
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                let anon = SlotKey::Anon(eid.0);
                for arm in [*then_branch, *else_branch] {
                    if let Some(k) = self.container_key_of(arm, scope) {
                        self.guarded_unions.push((anon.clone(), k));
                    }
                }
                Some(anon)
            }
            Expr::Nullish { lhs, rhs } => {
                let anon = SlotKey::Anon(eid.0);
                for arm in [*lhs, *rhs] {
                    if let Some(k) = self.container_key_of(arm, scope) {
                        self.guarded_unions.push((anon.clone(), k));
                    }
                }
                Some(anon)
            }
            Expr::Sequence { right, .. } => self.container_key_of(*right, scope),
            Expr::As { expr, .. } => self.container_key_of(*expr, scope),
            Expr::Assign { value, .. } => self.container_key_of(*value, scope),
            _ => None,
        }
    }

    fn callee_fn_name(&self, eid: ExprId) -> Option<String> {
        match self.ast.get_expr(eid) {
            Expr::Ident(n) if self.fn_params.contains_key(n) => Some(n.clone()),
            Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
            _ => None,
        }
    }

    /// Result container key of a member call, for flows consuming it.
    /// Transform plumbing (slice / concat / map …) is unconditional —
    /// nested element references alias through the result.
    fn method_result_key(
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
                    if let Some(ak) = self.container_key_of(a, scope) {
                        self.uf.union(&aek, &SlotKey::Elem(Box::new(ak)));
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
                if any { Some(anon) } else { None }
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
            if let Some(ak) = self.container_key_of(a, scope) {
                self.uf.union(&ek, &ak);
            }
        }
        // Callback-taking iteration — the element param sees the
        // receiver's elems (value flow + nested-reference alias).
        match name.as_str() {
            "map" | "forEach" | "filter" | "find" | "findLast" | "findIndex" | "some" | "every"
            | "flatMap" => {
                if let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a))
                    && let Some(p0) = self.fn_params.get(&cb).and_then(|ps| ps.first()).cloned()
                {
                    let pk = SlotKey::Param(cb, p0);
                    self.c_edges
                        .entry(ek.clone())
                        .or_default()
                        .push((pk.clone(), false));
                    self.guarded_unions.push((pk, ek.clone()));
                }
            }
            "reduce" | "reduceRight" => {
                if let Some(cb) = args.first().and_then(|a| self.callee_fn_name(*a))
                    && let Some(ps) = self.fn_params.get(&cb).cloned()
                {
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
        for c in self.classes.clone() {
            let f = format!("__cm_{c}__{name}");
            if let Some(ps) = self.fn_params.get(&f).cloned() {
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
    }

    /// Element / field write through an Assign target (`xs[i] = e`,
    /// `o.x = e`). An untrackable index-assign receiver poisons every
    /// container query — conservative, never silent-wrong.
    pub(super) fn container_assign(&mut self, target: ExprId, value: ExprId, scope: &Scope) {
        let t = self.ast.get_expr(target).clone();
        match &t {
            Expr::Index { obj, .. } => match self.container_key_of(*obj, scope) {
                Some(rk) => {
                    self.mark_containerish(&rk);
                    let ek = SlotKey::Elem(Box::new(rk));
                    let w = self.width_of(value, scope);
                    self.add_container_constraint(ek.clone(), w);
                    if let Some(vk) = self.container_key_of(value, scope) {
                        self.uf.union(&ek, &vk);
                    }
                }
                None => self.container_poison = true,
            },
            Expr::Member { obj, name } => {
                if let Some(rk) = self.container_key_of(*obj, scope) {
                    self.mark_containerish(&rk);
                    let fk = SlotKey::Field(Box::new(rk), name.clone());
                    let w = self.width_of(value, scope);
                    self.add_container_constraint(fk.clone(), w);
                    if let Some(vk) = self.container_key_of(value, scope) {
                        self.uf.union(&fk, &vk);
                    }
                }
                // Accessor route: `q.X = v` may lower to
                // `__cm_<C>__X_set(q, v)` — feed the value param of
                // every owner's setter.
                for c in self.classes.clone() {
                    let f = format!("__cm_{c}__{name}_set");
                    if let Some(p1) = self.fn_params.get(&f).and_then(|ps| ps.get(1)).cloned() {
                        let pk = SlotKey::Param(f.clone(), p1);
                        let w = self.width_of(value, scope);
                        self.add_container_constraint(pk.clone(), w);
                        self.alias_guarded(pk, value, scope);
                    }
                }
            }
            _ => {}
        }
    }
}
