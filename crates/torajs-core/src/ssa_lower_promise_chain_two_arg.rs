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
        let on_ok_pre_ty = self.operand_ty(&on_ok);
        let on_ok = self.maybe_wrap_promise_cb(on_ok);
        let on_err = self.lower_expr(args[1]);
        let on_err_pre_ty = self.operand_ty(&on_err);
        let on_err = self.maybe_wrap_promise_cb(on_err);
        let on_ok_ty = self.operand_ty(&on_ok);
        let on_err_ty = self.operand_ty(&on_err);
        let mut ok_word =
            self.chain_cb_repr_word(&on_ok_pre_ty) | self.chain_cb_param_repr(&on_ok_pre_ty);
        // The CLOSURE flag is the one bit that IS about the operand
        // the kernel receives, so it alone reads the post-wrap type.
        if matches!(on_ok_ty, Type::Closure(_)) {
            ok_word |= crate::ssa_lower_promise_repr_mark::THEN2_CLOSURE_FLAG;
        }
        let mut err_word =
            self.chain_cb_repr_word(&on_err_pre_ty) | self.chain_cb_param_repr(&on_err_pre_ty);
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
    /// call).
    ///
    /// **Takes the cb type BEFORE `maybe_wrap_promise_cb`**, and both
    /// halves need it: the wrap's signature is `(i64) -> i64`, which
    /// names neither end of the handler inside it. Asking the wrapped
    /// operand instead
    ///
    /// - lost the `any` parameter face of a handler wrapped for its
    ///   RETURN alone — `(v: any) => 0` on an f64 value read
    ///   `Promise.resolve(12.5)` back as `9`, exactly one
    ///   `DOUBLE_ENCODE_OFFSET` short, because the kernel skipped the
    ///   boxing an unflagged handler does not ask for;
    /// - reported the cb-leg result as I64 whatever the handler
    ///   actually returned, so a downstream any-lane reader of
    ///   `.then((v: number) => v * 2)` boxed raw f64 bits as an
    ///   integer, and of a string-returning handler boxed the Str
    ///   pointer as one.
    ///
    /// The wrap is transparent to both faces, which is why the
    /// pre-wrap answer is the true one: an `any` parameter is never
    /// the f64 face the adapter converts, so the value reaches the
    /// handler untouched; and the adapter's outbound bitcast leaves
    /// precisely the bits the pre-wrap return face names.
    pub(crate) fn chain_cb_repr_word(&self, pre_wrap_ty: &Type) -> i64 {
        let mut w = self.chain_cb_ret_repr(pre_wrap_ty);
        if let Type::Closure(sig) | Type::FnSig(sig) = pre_wrap_ty
            && matches!(self.fn_sigs[sig.0 as usize].0.first(), Some(Type::Any))
        {
            w |= crate::ssa_lower_promise_repr_mark::PARAM_ANY_FLAG;
        }
        w
    }

    /// RFC 20260727 — the lane the kernel must unbox INTO when the
    /// source cell was settled from an `any`. Only the kernel can
    /// decide that (the cell's stamp is the truth, and SSA's
    /// `Type::Promise` is inner-T erased); this hands it the target.
    ///
    /// **Takes the cb type BEFORE `maybe_wrap_promise_cb`.** That
    /// wrapper's own signature is `(i64) -> i64` — the f64 bits-ABI
    /// adapter — so asking the wrapped operand what its parameter is
    /// answers I64 for every f64-faced handler, and the kernel would
    /// hand a bit-identical integer to a thunk that bitcasts it: 42
    /// arriving as 2.08e-322.
    pub(crate) fn chain_cb_param_repr(&self, pre_wrap_ty: &Type) -> i64 {
        let (Type::Closure(sig) | Type::FnSig(sig)) = pre_wrap_ty else {
            return 0;
        };
        let Some(param) = self.fn_sigs[sig.0 as usize].0.first().cloned() else {
            return 0;
        };
        if param == Type::Any {
            return 0;
        }
        let as_f64 = matches!(param, Type::F64);
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&param, as_f64, false)
            .map(|r| r << crate::ssa_lower_promise_repr_mark::PARAM_REPR_SHIFT)
            .unwrap_or(0)
    }
}
