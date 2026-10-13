//! The Call arm of the constraint walk (`walk.rs`).

use super::{Analysis, Scope, SlotKey};
use crate::ast::PropKey;
use crate::ast::{BinOp, Expr, ExprId};

impl Analysis<'_> {
    /// The Call arm of [`Self::walk_expr`]: argument widths flow into
    /// the callee's parameter slots (a named fn's by name, a fn-typed
    /// container slot's by position), then the member-call effects and
    /// the operands themselves.
    pub(super) fn walk_call(
        &mut self,
        eid: ExprId,
        callee: ExprId,
        args: &[ExprId],
        scope: &Scope,
    ) {
        if self.char_code_in_bounds(callee, args) {
            self.char_code_proven.insert(eid);
        }
        if let Some(fname) = self.direct_callee(eid, callee)
            && let Some(pnames) = self.fn_params.get(&fname).cloned()
        {
            for (i, arg) in args.iter().enumerate() {
                if let Some(pname) = pnames.get(i) {
                    let w = self.flow(*arg, scope);
                    let pk = SlotKey::Param(fname.clone(), pname.clone());
                    self.add_constraint(pk.clone(), w);
                    self.alias_guarded(pk.clone(), *arg, scope);
                    self.fn_value_flow(&pk, *arg, scope);
                }
            }
            // a spread hands every later parameter one of its elements
            if let Some(at) = args
                .iter()
                .position(|a| matches!(self.ast.get_expr(*a), Expr::Spread { .. }))
            {
                let m = self.mag_of(args[at], scope);
                for p in pnames.into_iter().skip(at) {
                    self.record_mag(SlotKey::Param(fname.clone(), p), m.clone());
                }
            }
        } else if !matches!(self.ast.get_expr(callee), Expr::Member { .. })
            && let Some(ck) = self.container_key_lookup(callee, scope)
        {
            // F1 — indirect call through an fn value: args flow
            // into the value's `__p{i}` projections (the
            // indirect mirror of the direct Param edges; member
            // callees keep their builtin method handling).
            //
            // A callee that chooses among functions in place (`(c ?
            // f : g)(x)`, `(f || g)(x)`) has no binding whose init
            // glued them on: every function it may answer joins the
            // projections here — named functions directly, fn-valued
            // slots through the choice's own arm unions — so they all
            // share one signature with the single indirect call that
            // reaches them.
            if matches!(
                self.ast.get_expr(callee),
                Expr::Ternary { .. }
                    | Expr::Nullish { .. }
                    | Expr::BinOp {
                        op: BinOp::LAnd | BinOp::LOr,
                        ..
                    }
            ) {
                self.container_key_of(callee, scope);
            }
            self.fn_value_flow(&ck, callee, scope);
            self.record_variadic_call(callee, args, &ck);
            for (i, arg) in args.iter().enumerate() {
                let w = self.flow(*arg, scope);
                let pk = SlotKey::Field(Box::new(ck.clone()), PropKey::from(format!("__p{i}")));
                self.add_constraint(pk.clone(), w);
                // RFC 20260726-array-elem-width — a CONTAINER
                // argument has to join the projection too, not
                // just hand over its scalar width. The direct
                // arm above does this; without it here, an
                // arrow bound to a local (`let f = (xs:
                // number[]) => xs[0]`) left the caller's array
                // in a different class from the param reading
                // it, so the two disagreed on element width.
                // `fn_value_flow` at the binding's init already
                // glued this projection onto the lifted fn's
                // Param key, so one union reaches it.
                self.alias_guarded(pk.clone(), *arg, scope);
                self.fn_value_flow(&pk, *arg, scope);
            }
        }
        self.member_call_effects(eid, callee, args, scope);
        // a called member is not a value read of the method
        match self.ast.get_expr(callee) {
            Expr::Member { obj, .. } => self.walk_expr(*obj, scope),
            _ => self.walk_expr(callee, scope),
        }
        for a in args {
            self.walk_expr(*a, scope);
        }
    }
}
