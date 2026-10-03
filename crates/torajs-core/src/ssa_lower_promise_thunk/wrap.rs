//! The call-site half of [`super`]/`ssa_lower_promise_thunk`: wrap a
//! callback in the adapter the pre-scan synthesized for its faces.

use super::PTHUNK_ENV_SIZE;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{
    CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF,
    intern_fn_sig,
};

/// The call-site half of the adapters above: wrap a callback whose
/// negotiated signature carries an F64 face in its synthesized
/// bits-ABI thunk.
///
/// Lives beside the synthesis rather than with the `.then` / `.catch`
/// lowering because it is the same concern as the thunks it selects
/// from.
impl crate::ssa_lower::LowerCtx<'_> {
    /// ②.6b — wrap an f64-faced `.then` / `.catch` handler in its
    /// synthesized bits-ABI adapter. The promise runtime invokes
    /// every handler as `(env, i64) -> i64` raw bits; a callback
    /// whose negotiated signature carries an F64 face would cross
    /// that boundary in the wrong register bank. The wrap is a
    /// closure-shaped env (fn slot → thunk, capture 0 → the real
    /// callback) so the runtime's closure dispatcher works unchanged.
    /// Integral-faced callbacks pass through untouched. Answers `None`
    /// for those; `Some(wrap)` is a fresh +1 cell the caller settles.
    /// `cb_owned`: the call site owns a reference on a closure `cb_op`
    /// (a fresh temp) — it moves into the wrap; a borrowed one gets a
    /// reference of the wrap's own.
    pub(crate) fn wrap_promise_cb(&mut self, cb_op: Operand, cb_owned: bool) -> Option<Operand> {
        let cb_ty = self.operand_ty(&cb_op);
        let (sid, is_closure) = match cb_ty {
            Type::Closure(s) => (s, true),
            Type::FnSig(s) => (s, false),
            _ => return None,
        };
        let (params, ret) = self.fn_sigs[sid.0 as usize].clone();
        let p = params.first() == Some(&Type::F64);
        let r = ret == Type::F64;
        if !p && !r {
            return None;
        }
        let (thunk_fid, thunk_sig) = self.promise_thunks.get(is_closure, p, r)?;
        let drop_pair = if is_closure {
            self.promise_thunks.drop_closure
        } else {
            self.promise_thunks.drop_fnsig
        };
        let (drop_fid, drop_sig) = drop_pair?;
        let wrap_user_sig = intern_fn_sig(self.fn_sigs, vec![Type::I64], Type::I64);
        let wrap_ty = Type::Closure(wrap_user_sig);
        let env_v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.obj_alloc,
                vec![Operand::ConstI64(PTHUNK_ENV_SIZE)],
            ),
            wrap_ty,
            None,
        );
        // Universal heap header: refcount=1, type_tag=CLOSURE=3.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(3), Operand::Value(env_v), 4),
        );
        let thunk_addr = self.f.append_inst(
            self.cur_block,
            InstKind::FnAddr(thunk_fid),
            Type::FnSig(thunk_sig),
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(thunk_addr),
                Operand::Value(env_v),
                CLOSURE_FN_ADDR_OFF,
            ),
        );
        let drop_addr = self.f.append_inst(
            self.cur_block,
            InstKind::FnAddr(drop_fid),
            Type::FnSig(drop_sig),
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(drop_addr),
                Operand::Value(env_v),
                CLOSURE_DROP_FN_OFF,
            ),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::ConstI64(0),
                Operand::Value(env_v),
                CLOSURE_PROPS_OFF,
            ),
        );
        // boxed_entry — the pthunk wrap is promise-internal, never
        // reachable as an any-world callee; stays 0.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::ConstI64(0),
                Operand::Value(env_v),
                crate::ssa_lower::CLOSURE_BOXED_ENTRY_OFF,
            ),
        );
        // trace_fn — closure inners store the shared cap0 trace so
        // the collector walks the wrapped-callback edge (RFC 20260717
        // residual ①); fnsig inners hold a raw code address in cap0
        // (not a cell — a code address could even pass the collector's
        // cell-like gate) and keep 0. obj_alloc is plain malloc, so
        // the 0 must be stored explicitly either way.
        let trace_op = match self.promise_thunks.trace_closure {
            Some((tfid, tsig)) if is_closure => Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::FnAddr(tfid),
                Type::FnSig(tsig),
                None,
            )),
            _ => Operand::ConstI64(0),
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                trace_op,
                Operand::Value(env_v),
                crate::ssa_lower::CLOSURE_TRACE_FN_OFF,
            ),
        );
        // Capture 0: the real callback, one reference of the wrap's
        // own (the wrap's drop releases it).
        if is_closure && !cb_owned {
            self.emit_rc_inc(cb_op);
        }
        self.f.append_void(
            self.cur_block,
            InstKind::Store(cb_op, Operand::Value(env_v), CLOSURE_CAP_BASE_OFF),
        );
        Some(Operand::Value(env_v))
    }
}
