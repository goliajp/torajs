//! W4 / F5 — the width wiring a Promise call site contributes.
//!
//! Split out of `container_methods.rs` under the 500-line discipline.
//! That file answers "what does a member call do to the widths around
//! it" for containers; the Promise statics and the `.then` chain are
//! the one receiver family whose answer is about a value travelling
//! through a settle rather than about elements, and they were the
//! largest block that says so.

use super::{Analysis, Scope, SlotKey};
use crate::ast::{Ast, Expr, ExprId, PropKey};
use std::collections::HashMap;

/// `await p` parses to `p.value`, so this answers the inner type of
/// the promise being awaited (`None` for any other expression).
///
/// A promise's value slot carries whatever settled it, and an async
/// body that runs off its end settles with that width's undefined
/// sentinel (ES §10.2.1.4 step 11) — as does any sentinel-bearing
/// value handed to `Promise.resolve`. Over-broad for the promises
/// that always settle with a real value: one predictable compare,
/// never wrong.
///
/// It lives here, in the module about the Promise value point, rather
/// than beside either caller, because BOTH phases need it and they
/// need it to mean the same thing. Lowering asks in order to arm the
/// read; this analysis asks in order to give the slot the read lands
/// in the width an armed read requires. Those two answers disagreeing
/// is not a small bug — arming a slot that is not wide enough to hold
/// the sentinel is a silent no-op (RFC 20260923 §3.3c), and it looks
/// exactly like the table having missed the value.
pub(crate) fn awaited_promise_inner_of<'t>(
    ast: &Ast,
    expr_types: &'t HashMap<ExprId, crate::check::Type>,
    eid: ExprId,
) -> Option<&'t crate::check::Type> {
    let Expr::Member { obj, name } = ast.get_expr(eid) else {
        return None;
    };
    if name != "value" {
        return None;
    }
    match expr_types.get(obj) {
        Some(crate::check::Type::Promise(inner)) => Some(inner),
        _ => None,
    }
}

impl Analysis<'_> {
    /// ②.6b — value-point wiring for the Promise statics. The
    /// single-value statics (`resolve` / `reject`) feed the argument's
    /// width into the result's `value` point; thenable absorption
    /// passes the inner promise's point through. The combinators
    /// (`all` / `race` / `any`) fan the source elements' `value`
    /// points into the result's: `all` resolves to an ARRAY of those
    /// values (Elem of the result point), `race` / `any` to ONE of
    /// them (the result point itself). `allSettled` resolves to an
    /// array of `{status, value}` records whose `value` field holds
    /// what each source settled with, so that field joins the source
    /// points (the rejected `{status, reason}` twin shares its layout).
    /// Fired from member_call_effects (every call site), keyed by the
    /// call's Anon origin.
    pub(super) fn promise_static_wiring(
        &mut self,
        eid: ExprId,
        name: &str,
        args: &[ExprId],
        scope: &Scope,
    ) {
        let anon = SlotKey::Anon(eid.0);
        self.mark_containerish(&anon);
        let pv = SlotKey::Field(Box::new(anon.clone()), "value".into());
        if matches!(name, "resolve" | "reject")
            && let Some(a0) = args.first()
        {
            let w = self.flow(*a0, scope);
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
        if matches!(name, "all" | "race" | "any" | "allSettled")
            && let Some(a0) = args.first()
            && let Some(ak) = self.container_key_of(*a0, scope)
        {
            let src_elem_pv = SlotKey::Field(Box::new(SlotKey::Elem(Box::new(ak))), "value".into());
            self.combinator_wiring(name, &pv, &src_elem_pv);
        }
    }

    /// A combinator call's result value point `pv` against the source
    /// elements' value point.
    fn combinator_wiring(&mut self, name: &str, pv: &SlotKey, src_elem_pv: &SlotKey) {
        if name == "allSettled" {
            self.mark_containerish(pv);
            let rec = SlotKey::Elem(Box::new(pv.clone()));
            self.mark_containerish(&rec);
            self.uf
                .union(&SlotKey::Field(Box::new(rec), "value".into()), src_elem_pv);
        } else if name == "all" {
            // The result's value is a fresh array the runtime fills
            // from each source promise's raw value slot — container
            // evidence on the point itself, so the
            // `let r = await Promise.all(...)` guarded copy edge
            // activates.
            self.mark_containerish(pv);
            self.uf
                .union(&SlotKey::Elem(Box::new(pv.clone())), src_elem_pv);
        } else {
            self.uf.union(pv, src_elem_pv);
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
    pub(super) fn promise_chain_wiring(
        &mut self,
        eid: ExprId,
        recv: &SlotKey,
        name: &str,
        args: &[ExprId],
        scope: &Scope,
    ) {
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
                    if p_num && let Some(p0) = self.callback_params(&cb).first() {
                        self.uf
                            .union(&src_pv, &SlotKey::Param(cb.clone(), p0.clone()));
                        self.any_number_reader(eid, *a, &src_pv);
                    }
                    if r_num {
                        self.uf.union(&res_pv, &SlotKey::Ret(cb));
                    }
                } else if let Some(pk) = self.callback_projection(*a, "__p0", scope) {
                    // a handler read out of a slot: only the any lane
                    // converts the value through its boxed entry; a
                    // typed lane hands raw bits through a bitcast
                    // thunk and an inline await reads the slot raw, so
                    // each face widens with its value point both ways.
                    // Edges rather than a union: the slot's projection
                    // class may hold handlers whose faces are not
                    // numbers, which must not join the value class.
                    self.both_ways(&src_pv, &pk);
                    if let Some(rk) = self.callback_projection(*a, "__ret", scope) {
                        self.both_ways(&res_pv, &rk);
                    }
                }
            }
        }
    }

    /// A handler `cb` declaring a `number` parameter on a promise of
    /// `any` (the receiver of the chain call `eid` is a `.then` whose
    /// handler returned `any`, or a `Promise<any>`) takes whatever the
    /// `any` held, so the source value point `src_pv` widens.
    fn any_number_reader(&mut self, eid: ExprId, cb: ExprId, src_pv: &SlotKey) {
        use crate::check::Type;
        let Expr::Call { callee, .. } = self.ast.get_expr(eid) else {
            return;
        };
        let Expr::Member { obj, .. } = self.ast.get_expr(*callee) else {
            return;
        };
        let any_src =
            matches!(self.expr_types.get(obj), Some(Type::Promise(t)) if **t == Type::Any);
        let num_param = matches!(self.expr_types.get(&cb),
            Some(Type::Function(ps, _)) if ps.first() == Some(&Type::Number));
        if any_src && num_param {
            self.add_container_constraint(src_pv.clone(), super::magnitude::f64_flow());
        }
    }

    /// The class getters among the function `fname` of the declared
    /// return type `r`, when that is a promise: see
    /// [`Self::promise_getter_union`].
    pub(super) fn promise_getter_unions(&mut self, fname: &str, r: &str) {
        if !r.starts_with("Promise<") {
            return;
        }
        let ast = self.ast;
        for ((class, prop), _) in ast.accessor_getters.iter().filter(|(_, f)| *f == fname) {
            let ret = SlotKey::Ret(fname.to_string());
            self.promise_getter_union(&SlotKey::Class(class.clone()), prop.clone(), ret);
        }
    }

    /// The field `fname` of the object literal `lit`, stored at `fk`
    /// with the value `fe`, when it is a getter (`__getter_<prop>`)
    /// returning a promise: see [`Self::promise_getter_union`].
    pub(super) fn objlit_promise_getter(
        &mut self,
        lit: &SlotKey,
        fname: &PropKey,
        fk: &SlotKey,
        fe: ExprId,
    ) {
        use crate::check::Type;
        if let Some(prop) = fname.lossy().strip_prefix("__getter_")
            && let Some(Type::Function(_, r)) = self.expr_types.get(&fe)
            && matches!(**r, Type::Promise(_))
        {
            let ret = SlotKey::Field(Box::new(fk.clone()), "__ret".into());
            self.promise_getter_union(lit, prop.into(), ret);
        }
    }

    /// A getter answers the reads of its property, so a promise it
    /// returns settles the value point those reads await: `ret` (the
    /// getter's result) joins the property `prop` of `holder` (its
    /// class, or the object literal that holds it).
    pub(super) fn promise_getter_union(&mut self, holder: &SlotKey, prop: PropKey, ret: SlotKey) {
        let fk = SlotKey::Field(Box::new(holder.clone()), prop);
        self.mark_containerish(holder);
        self.mark_containerish(&fk);
        self.mark_containerish(&ret);
        self.uf.union(&fk, &ret);
    }

    /// Width edges `a → b` and `b → a`, neither marked growth: a value
    /// handed across unchanged cannot close a growth cycle.
    fn both_ways(&mut self, a: &SlotKey, b: &SlotKey) {
        self.c_edges
            .entry(a.clone())
            .or_default()
            .push((b.clone(), false));
        self.c_edges
            .entry(b.clone())
            .or_default()
            .push((a.clone(), false));
    }
}
