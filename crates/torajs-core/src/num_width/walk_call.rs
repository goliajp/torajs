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
            self.flow_into_projections(&ck, args, scope);
        } else if let Some((obj, pos)) = self.replayed_args(callee, args)
            && let Some(ck) = self.container_key_lookup(obj, scope)
        {
            // `f.call(t, ...)` / `f.apply(t, ...)` on an fn value: the
            // arguments after the receiver fill its projections as a
            // plain indirect call's would; an `apply` array that is no
            // literal lists none of them
            self.fn_value_flow(&ck, obj, scope);
            match pos {
                Some(pos) => self.flow_into_projections(&ck, &pos, scope),
                None => self.record_unlisted_args(&ck, 0),
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

    /// Each of `args` into the `__p{i}` projection of the fn value
    /// slot `ck`, by position. A spread stands for the arguments from
    /// its position on, which no projection lists.
    fn flow_into_projections(&mut self, ck: &SlotKey, args: &[ExprId], scope: &Scope) {
        let spread = |a: &ExprId| matches!(self.ast.get_expr(*a), Expr::Spread { .. });
        if let Some(at) = args.iter().position(spread) {
            self.record_unlisted_args(ck, at);
        }
        for (i, arg) in args.iter().enumerate() {
            let w = self.flow(*arg, scope);
            let pk = SlotKey::Field(Box::new(ck.clone()), PropKey::from(format!("__p{i}")));
            self.add_constraint(pk.clone(), w);
            // a container argument joins the projection too, not just
            // hands over its scalar width: an arrow bound to a local
            // (`let f = (xs: number[]) => xs[0]`) otherwise read the
            // caller's array from a different class than the param
            // reading it, and the two disagreed on element width.
            // `fn_value_flow` at the binding's init already glued this
            // projection onto the lifted fn's Param key, so one union
            // reaches it.
            self.alias_guarded(pk.clone(), *arg, scope);
            self.fn_value_flow(&pk, *arg, scope);
        }
    }

    /// For `f.call(t, a, b)` and `f.apply(t, [a, b])` the function `f`
    /// and `Some([a, b])`; for `f.apply(t, xs)`, whose arguments are
    /// not written out, `f` and `None`.
    fn replayed_args(
        &self,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<(ExprId, Option<Vec<ExprId>>)> {
        let Expr::Member { obj, name } = self.ast.get_expr(callee) else {
            return None;
        };
        match (name.as_str(), args.get(1).map(|a| self.ast.get_expr(*a))) {
            ("call", _) => Some((*obj, Some(args.iter().skip(1).copied().collect()))),
            ("apply", None) => Some((*obj, Some(Vec::new()))),
            ("apply", Some(Expr::Array(els))) => Some((*obj, Some(els.clone()))),
            ("apply", Some(_)) => Some((*obj, None)),
            _ => None,
        }
    }
}
