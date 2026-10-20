//! W4 — constraint-walk hooks for the container face: container-key
//! resolution and member-call element effects (element / field
//! assignment constraints: `container_assign.rs`). The union-find /
//! activation / congruence machinery these feed lives in
//! `container.rs`.

use super::{Analysis, Scope, SlotKey, W};
use crate::ast::PropKey;
use crate::ast::{BinOp, Expr, ExprId};

impl<'a> Analysis<'a> {
    /// Container-channel constraint — same shape as `add_constraint`
    /// but lands in `c_seeds`/`c_edges`, which only the canonical
    /// fixpoint merges in. Keeps the scalar (pre-W4) fixpoint
    /// bit-identical until D2/D3 wire the consuming lowering sites.
    pub(super) fn add_container_constraint(
        &mut self,
        target: SlotKey,
        (w, m): super::magnitude::Flow,
    ) {
        self.record_write(target.clone(), &w, m);
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
    /// fixpoint absorbs. A value whose type holds `any` is noted as a
    /// source of promises the walk does not follow (`promise_entry`).
    pub(super) fn container_key_of(&mut self, eid: ExprId, scope: &Scope) -> Option<SlotKey> {
        let k = self.container_key_shape(eid, scope)?;
        self.note_untracked_value(eid, &k, scope);
        Some(k)
    }

    /// [`Self::container_key_of`] before the value is noted as a source.
    fn container_key_shape(&mut self, eid: ExprId, scope: &Scope) -> Option<SlotKey> {
        let e = self.ast.get_expr(eid).clone();
        match &e {
            Expr::Ident(n) => {
                if let Some(k) = self.resolve(n, scope) {
                    return Some(k);
                }
                // Synthetic body, name with no pinned binding
                // (`captures.rs`) — alias every same-named slot through
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
                // The `this.__sent` an async body's `let v = await e`
                // became after the move is `e`'s settled value — read
                // it as `e.value`, the promise's value point, or the
                // binding sits in a class of its own and an array it
                // received reads back at the literal's width
                // (`arr-promise-elem-width-001`, un-moved until now).
                if let Some(origin) = self.ast.yield_into_await_origins.get(&eid).copied() {
                    let base = self.container_key_of(origin, scope)?;
                    return Some(SlotKey::Field(Box::new(base), "value".into()));
                }
                let base = self.container_key_of(*obj, scope)?;
                Some(SlotKey::Field(Box::new(base), PropKey::from(name)))
            }
            Expr::OptIndex { obj, .. } | Expr::Index { obj, .. } => {
                let base = self.container_key_of(*obj, scope)?;
                Some(SlotKey::Elem(Box::new(base)))
            }
            Expr::Call { callee, args } => {
                if let Some(f) = self.direct_callee(eid, *callee)
                    && self.fn_params.contains_key(&f)
                {
                    return Some(SlotKey::Ret(f));
                }
                // Not a lifted fn's own name — an fn-valued binding
                // (`const a = (n) => …; a(1)`, or a parameter that
                // shadows a declaration) or an unknown global falls
                // through to the indirect projection below.
                if !self.retargets.contains_key(&eid)
                    && let Expr::Member { obj, name } = self.ast.get_expr(*callee).clone()
                {
                    return self.method_result_key(eid, obj, &name, args, scope);
                }
                // F1 mirror — indirect call through an fn value: the
                // result reads the value's `__ret` projection, the
                // same spelling `width_of_call`'s `_` arm and the
                // walk's `__p{i}` arg edges already use.
                // `fn_value_flow` at the binding's init glued that
                // projection onto the lifted fn's Ret key, so one
                // union reaches it. Without this the result had NO
                // container key: `let r: number[] = a(1)` never
                // joined `a`'s ret class, `a(boom())` widened the
                // callee's elements to f64, and `r` stayed narrow —
                // a reassignment died on the slot-fit mismatch while
                // the init form bit-punned f64 slots as integers.
                self.container_key_of(*callee, scope)
                    .map(|k| SlotKey::Field(Box::new(k), "__ret".into()))
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
                    let w = self.flow(el, scope);
                    self.add_container_constraint(ek.clone(), w);
                    // A function value as a literal element: same
                    // projection wiring as a `push` (638-13).
                    self.fn_value_flow(&ek, el, scope);
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
                    self.add_container_constraint(fk.clone(), self.flow(fe, scope));
                    // F5 — a function value stored in a struct-literal
                    // field wires the field's `__ret` / `__p{i}`
                    // projections onto the function's Ret / Param keys,
                    // so the struct-field-fn dispatch face answers
                    // width queries from the resident's own class.
                    self.fn_value_flow(&fk, fe, scope);
                    self.objlit_promise_getter(&anon, &fname, &fk, fe);
                    if let Some(ik) = self.container_key_of(fe, scope) {
                        // F2-fix — same candidate-side gate as the
                        // ArrayLit elems above.
                        self.nested_unions.push((fk.clone(), ik));
                    }
                }
                Some(anon)
            }
            Expr::Ternary {
                then_branch: lhs,
                else_branch: rhs,
                ..
            }
            // `a || b` / `a && b` answer one of their operands like
            // `??` does (§13.13), so the join holds either one
            | Expr::Nullish { lhs, rhs }
            | Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left: lhs,
                right: rhs,
            } => {
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

    /// An index read can go out of bounds, where ES §10.4.2.1 answers
    /// `undefined` — so being read by index is itself a reason to widen
    /// a `number` element slot, the same way a fractional write is.
    ///
    /// An I64 slot has no bit pattern to spare for that answer, which is
    /// why the lowering raises a RangeError there instead
    /// (`ssa_lower_index::lower_typed_index_checked`) — loud, but not
    /// what bun does, and inside a named fn the pending throw used to
    /// unwind the body while the caller's check was M4.3.b-skipped. F64
    /// has the sentinel, and the consuming side already assumes it:
    /// `is_undef_f64_source` routes every `number[]` index read through
    /// the sentinel compare.
    pub(super) fn seed_index_read_elem(&mut self, obj: ExprId, scope: &Scope, in_bounds: bool) {
        let number_elems = matches!(
            self.expr_types.get(&obj),
            Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::Number
        );
        // A method product read in place (`xs.concat([…])[1](3)`) has
        // no binding whose init resolves its key, and resolving it is
        // what fires the result-key wiring (`method_result_key`): the
        // concat arm joins the argument literal's element class onto
        // the receiver's, so a closure in that literal takes the
        // receiver's widened signature. Unresolved, the closure
        // compiled with integer faces while the product's element
        // slot — the receiver's layout — was called with float ones
        // (638-13, rotation 640). The read is the product's consumer,
        // so the read resolves it.
        let product_read = matches!(self.ast.get_expr(obj), Expr::Call { .. });
        if !number_elems && !product_read {
            return;
        }
        if let Some(rk) = self.container_key_of(obj, scope) {
            self.mark_containerish(&rk);
            if number_elems && !in_bounds {
                self.c_seeds.push(SlotKey::Elem(Box::new(rk)));
            }
        }
    }
}
