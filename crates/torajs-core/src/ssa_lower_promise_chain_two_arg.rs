//! Two-arg `.then(onOk, onErr)` lowering + the chain callback-repr
//! helper — split out of [`crate::ssa_lower_promise_chain`] (RFC
//! 20260720-anylane-promise-methods knife 3 pushed that file past
//! the 500-line cap; bodies verbatim).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// T-19.l — 2-arg `.then(onOk, onErr)` form is spec equivalent
    /// of `.then(onOk).catch(onErr)`. Lower as a chained pair of
    /// helper calls; the intermediate Promise is the bridge between
    /// the two stages and gets dropped after the catch attaches
    /// (`.catch` inc's its source's rc, so the drop balances the
    /// chain's natural ref).
    pub(crate) fn lower_then_two_arg(&mut self, src_op: Operand, args: &[ExprId]) -> ValueId {
        let on_ok = self.lower_expr(args[0]);
        let on_ok = self.maybe_wrap_promise_cb(on_ok);
        let on_err = self.lower_expr(args[1]);
        let on_err = self.maybe_wrap_promise_cb(on_err);
        let on_ok_ty = self.operand_ty(&on_ok);
        let ok_repr = self.chain_cb_ret_repr(&on_ok_ty);
        let then_fid = if matches!(on_ok_ty, Type::Closure(_)) {
            self.intrinsics.promise_then_closure
        } else {
            self.intrinsics.promise_then_simple
        };
        let mid = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                then_fid,
                vec![src_op.clone(), on_ok, Operand::ConstI64(ok_repr)],
            ),
            Type::Promise,
            None,
        );
        // ②.6b — pick the catch dispatcher by the (possibly
        // wrapped) handler's shape.
        let on_err_ty = self.operand_ty(&on_err);
        let err_repr = self.chain_cb_ret_repr(&on_err_ty);
        let catch_fid = if matches!(on_err_ty, Type::Closure(_)) {
            self.intrinsics.promise_catch_closure
        } else {
            self.intrinsics.promise_catch_simple
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                catch_fid,
                vec![Operand::Value(mid), on_err, Operand::ConstI64(err_repr)],
            ),
            Type::Promise,
            None,
        );
        self.emit_drop_value(Operand::Value(mid), Type::Promise);
        v
    }

    /// knife 3 (RFC 20260720-anylane-promise-methods) — the call
    /// site's static callback-return repr, handed to the then/catch
    /// kernel for its cb-leg result stamp (the forward leg copies
    /// the source's stamp kernel-side). 0 = UNSTAMPED for a return
    /// form the any-lane bridge doesn't decode — stays loud there.
    pub(crate) fn chain_cb_ret_repr(&self, cb_ty: &Type) -> i64 {
        let ret = match cb_ty {
            Type::Closure(sig) | Type::FnSig(sig) => self.fn_sigs[sig.0 as usize].1.clone(),
            _ => return 0,
        };
        let as_f64 = matches!(ret, Type::F64);
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&ret, as_f64, false).unwrap_or(0)
    }
}
