//! W4 / F5 — the width wiring a Promise call site contributes.
//!
//! Split out of `container_methods.rs` under the 500-line discipline.
//! That file answers "what does a member call do to the widths around
//! it" for containers; the Promise statics and the `.then` chain are
//! the one receiver family whose answer is about a value travelling
//! through a settle rather than about elements, and they were the
//! largest block that says so.

use super::{Analysis, Scope, SlotKey};
use crate::ast::ExprId;

impl Analysis<'_> {
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
    pub(super) fn promise_chain_wiring(
        &mut self,
        eid: ExprId,
        recv: &SlotKey,
        name: &str,
        args: &[ExprId],
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
                    }
                    if r_num {
                        self.uf.union(&res_pv, &SlotKey::Ret(cb));
                    }
                }
            }
        }
    }
}
