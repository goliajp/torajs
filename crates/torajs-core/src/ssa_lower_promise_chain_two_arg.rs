//! Two-arg `.then(onOk, onErr)` lowering + the chain callback-repr
//! helper — split out of [`crate::ssa_lower_promise_chain`] (RFC
//! 20260720-anylane-promise-methods knife 3 pushed that file past
//! the 500-line cap; bodies verbatim).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// 2-arg `.then(onOk, onErr)` — ONE native kernel attach
    /// carrying both handlers (`__torajs_promise_then2`,
    /// rotation 184). The old T-19.l `then(onOk) → mid →
    /// catch(onErr)` chain was value-equivalent but ran a rejected
    /// source's onErr two microtasks late (the rejection forwarded
    /// through `mid` first); §27.2.5.4 attaches both handlers to
    /// the SAME promise in one registration. Each handler word:
    /// low byte = return repr, bit 8 = PARAM_ANY, bit 9 =
    /// THEN2_CLOSURE (env block vs bare fn address).
    pub(crate) fn lower_then_two_arg(&mut self, src_op: Operand, args: &[ExprId]) -> ValueId {
        let on_ok = self.lower_expr(args[0]);
        let on_ok = self.maybe_wrap_promise_cb(on_ok);
        let on_err = self.lower_expr(args[1]);
        let on_err = self.maybe_wrap_promise_cb(on_err);
        let on_ok_ty = self.operand_ty(&on_ok);
        let on_err_ty = self.operand_ty(&on_err);
        let mut ok_word = self.chain_cb_repr_word(&on_ok_ty);
        if matches!(on_ok_ty, Type::Closure(_)) {
            ok_word |= crate::ssa_lower_promise_repr_mark::THEN2_CLOSURE_FLAG;
        }
        let mut err_word = self.chain_cb_repr_word(&on_err_ty);
        if matches!(on_err_ty, Type::Closure(_)) {
            err_word |= crate::ssa_lower_promise_repr_mark::THEN2_CLOSURE_FLAG;
        }
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.promise_then2,
                vec![
                    src_op,
                    on_ok,
                    Operand::ConstI64(ok_word),
                    on_err,
                    Operand::ConstI64(err_word),
                ],
            ),
            Type::Promise,
            None,
        );
        // RFC 20260720-promise-any-cb knife 1 — an any-param handler
        // can refuse an UNSTAMPED source at attach; the throw leaves
        // the result null, so a plain check suffices here.
        if (ok_word | err_word) & crate::ssa_lower_promise_repr_mark::PARAM_ANY_FLAG != 0 {
            self.emit_throw_check(None);
        }
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

    /// RFC 20260720-promise-any-cb knife 1 — the kernels' full repr
    /// word: low byte the cb-return repr (knife 3 above), bit 8 set
    /// when the cb's first parameter is `any` (the kernel boxes the
    /// settled value per the source cell's repr stamp before the
    /// call). Judged on the post-wrap type — a pthunk-wrapped f64
    /// face carries an I64 sig, so the two adapters stay mutually
    /// exclusive by construction.
    pub(crate) fn chain_cb_repr_word(&self, cb_ty: &Type) -> i64 {
        let mut w = self.chain_cb_ret_repr(cb_ty);
        if let Type::Closure(sig) | Type::FnSig(sig) = cb_ty
            && matches!(self.fn_sigs[sig.0 as usize].0.first(), Some(Type::Any))
        {
            w |= crate::ssa_lower_promise_repr_mark::PARAM_ANY_FLAG;
        }
        w
    }
}
