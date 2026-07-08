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
            Expr::OptIndex { obj, .. } | Expr::Index { obj, .. } => {
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
                        // F2-fix — a container stored as an element is
                        // aliased by the slot; a scalar element keeps
                        // the one-way width edge above (gluing it in
                        // closed growth cycles through literal elems —
                        // `[n, n*10]` floated the cb param).
                        self.nested_unions.push((ek.clone(), ik));
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
                    // F5 — a function value stored in a struct-literal
                    // field wires the field's `__ret` / `__p{i}`
                    // projections onto the function's Ret / Param keys,
                    // so the struct-field-fn dispatch face answers
                    // width queries from the resident's own class.
                    self.fn_value_flow(&fk, fe, scope);
                    if let Some(ik) = self.container_key_of(fe, scope) {
                        // F2-fix — same candidate-side gate as the
                        // ArrayLit elems above.
                        self.nested_unions.push((fk.clone(), ik));
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
                    // F5 — fn value assigned into a field: same sig
                    // projection wiring as the struct-literal fill.
                    self.fn_value_flow(&fk, value, scope);
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
