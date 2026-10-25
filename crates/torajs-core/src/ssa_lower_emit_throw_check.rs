//! Throw-check emission helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 393 — Path A.3-batch14.
//!
//! Single method:
//!
//! - `emit_throw_check(target)` — load the `throw_active` flag after
//!   a call; if non-zero, branch to the innermost active try-block's
//!   catch (via `try_stack`) or — if no try is active in this fn —
//!   emit drops + ret a sentinel so the caller's own throw_check picks
//!   it up. Skips entirely for runtime intrinsics (they never throw)
//!   and for verified-non-throwing user fns (M4.3.b — fib40 / gcd /
//!   mandelbrot etc., recovering the M4.1 5% slowdown for programs
//!   that never touch try/throw). For `main`, the escaped-throw path
//!   routes through `__torajs_uncaught_exit` to report the
//!   pending throw to stderr + exit 1 (bug-327 C2.5).
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so the
//! numerous per-call-site invocations from the various lower_expr call
//! arms need zero edits.

mod path;
mod temp_drop;

use crate::ast::ExprId;
use crate::ssa::{FuncId, IPred, InstKind, Operand, THROW_ACTIVE_SYM, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_temp_scratch::ThrowTemp;

impl<'a> LowerCtx<'a> {
    /// Inline read of the in-flight-throw flag — `GlobalRef` to the
    /// C-named static `torajs-throw` exports + one `Load`, answering
    /// the I64 the old `__torajs_throw_check()` call answered. A
    /// throw check follows every call that may raise, so on a hot
    /// loop the call itself was the cost (~10% of `class-method`,
    /// rotation 470). The e-graph never CSEs a `Load` across calls
    /// (only arithmetic is pure), so every check still observes the
    /// latest write; `self_tail_call`'s shape matcher accepts this
    /// two-inst probe alongside the legacy call.
    pub(crate) fn emit_throw_active_load(&mut self) -> ValueId {
        let flag_ptr = self.f.append_inst(
            self.cur_block,
            InstKind::GlobalRef(THROW_ACTIVE_SYM.to_string()),
            Type::Ptr,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(flag_ptr), 0),
            Type::I64,
            None,
        )
    }

    /// load the throw_active flag; if non-zero, branch to the innermost
    /// active try-block's catch (via `try_stack`) or — if no try is
    /// active in this fn — emit drops + ret a sentinel so the caller's
    /// own throw_check picks it up. Skips entirely for runtime intrinsics
    /// (they never throw).
    pub(crate) fn emit_throw_check(&mut self, target: Option<FuncId>) {
        self.emit_throw_check_inner(target, None);
    }

    /// [`Self::emit_throw_check`] for calls whose OWNED result is
    /// already materialized when the check runs: the throw path
    /// (catch branch AND propagate branch) drops `owned` before
    /// leaving, since neither destination can know about a value
    /// that never reached a local. Mint-and-throw kernels (e.g.
    /// `matchAll` on a non-`g` regex answers a fresh empty array
    /// alongside the pending TypeError) stranded one cell per
    /// caught throw without this.
    pub(crate) fn emit_throw_check_owned(
        &mut self,
        target: Option<FuncId>,
        owned: Operand,
        ty: Type,
    ) {
        self.emit_throw_check_inner(target, Some((owned, ty)));
    }

    fn emit_throw_check_inner(&mut self, target: Option<FuncId>, owned: Option<(Operand, Type)>) {
        if let Some(fid) = target {
            if self.is_intrinsic(fid) {
                return;
            }
            // M4.3.b — skip the check entirely if the callee is a
            // verified-non-throwing user fn. fib40 / popcount / gcd /
            // mandelbrot etc. all live here, so the M4.1 5% slowdown
            // is gone for any program that doesn't use try/throw at
            // all (or whose hot fns provably can't reach a throw).
            if !self.may_throw_fns.contains(&self.f_name_of(fid)) {
                return;
            }
        }
        let active = self.emit_throw_active_load();
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let normal_blk = self.f.add_block();
        let throw_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: throw_blk,
                else_blk: normal_blk,
            },
        );
        self.emit_throw_path(throw_blk, owned);
        self.cur_block = normal_blk;
    }

    /// Park an owned temp for the duration of a may-throw region —
    /// see `TempScratch::throw_live`. Returns the slot token for the
    /// matching [`Self::pop_throw_temp`].
    pub(crate) fn push_throw_temp(&mut self, op: Operand, ty: Type) -> usize {
        self.temps.throw_live.push(Some(ThrowTemp::Value(op, ty)));
        self.temps.throw_live.len() - 1
    }

    /// Park a relocation slot — its current pointee is the temp (see
    /// `ThrowTemp::DynobjSlot`).
    pub(crate) fn push_throw_slot(&mut self, slot: ValueId) -> usize {
        self.temps
            .throw_live
            .push(Some(ThrowTemp::DynobjSlot(slot)));
        self.temps.throw_live.len() - 1
    }

    /// Park a typed alloca whose current pointee is the temp (see
    /// `ThrowTemp::Slot`); no-op token for a Copy type.
    pub(crate) fn push_throw_typed_slot(&mut self, slot: ValueId, ty: Type) -> Option<usize> {
        if ty.is_copy() {
            return None;
        }
        self.temps.throw_live.push(Some(ThrowTemp::Slot(slot, ty)));
        Some(self.temps.throw_live.len() - 1)
    }

    /// Park a HOF dst slot; `deferred_len` is the pre-reserve state's
    /// running-count alloca when the cell's own length word is stale
    /// until the loop exit (see `ThrowTemp::ArrSlotDeferLen`).
    pub(crate) fn push_throw_arr_slot(
        &mut self,
        slot: ValueId,
        ty: Type,
        deferred_len: Option<ValueId>,
    ) -> Option<usize> {
        let Some(len_slot) = deferred_len else {
            return self.push_throw_typed_slot(slot, ty);
        };
        self.temps
            .throw_live
            .push(Some(ThrowTemp::ArrSlotDeferLen(slot, ty, len_slot)));
        Some(self.temps.throw_live.len() - 1)
    }

    /// Unpark — the temp's normal-path release site takes over from
    /// here. Unpark order may differ from park order (dead slots
    /// stay as `None` holes until the tail shrinks past them).
    pub(crate) fn pop_throw_temp(&mut self, token: usize) {
        debug_assert!(self.temps.throw_live[token].is_some());
        self.temps.throw_live[token] = None;
        while matches!(self.temps.throw_live.last(), Some(None)) {
            self.temps.throw_live.pop();
        }
    }

    /// The `(operand, type)` to park iff `eid`'s lowered value is an
    /// owned temp — same predicate family `release_owned_temp` uses
    /// on the normal path, so park and release stay in agreement.
    pub(crate) fn throw_temp_of(&self, eid: ExprId, op: &Operand) -> Option<(Operand, Type)> {
        if !self.expr_owned_shape(eid) && !self.expr_minted_closure(eid, op) {
            return None;
        }
        let ty = self.operand_ty(op);
        if ty.is_copy() {
            return None;
        }
        Some((*op, ty))
    }

    /// Park `eid`'s lowered value iff it is an owned temp (per
    /// [`Self::throw_temp_of`]) — the receiver / callee a consumer
    /// holds while it lowers the arguments. Pairs with
    /// [`Self::unpark_owned_temp`] right after the consuming call.
    pub(crate) fn park_owned_temp(&mut self, eid: ExprId, op: &Operand) -> Option<usize> {
        self.throw_temp_of(eid, op)
            .map(|(op, ty)| self.push_throw_temp(op, ty))
    }

    pub(crate) fn unpark_owned_temp(&mut self, token: Option<usize>) {
        if let Some(t) = token {
            self.pop_throw_temp(t);
        }
    }

    /// The throw check of a kernel call that borrowed `eid`'s value as
    /// an argument, releasing the argument's owned temp on both edges:
    /// the throw edge through the parked slot, the normal edge here.
    /// A check followed by the release alone leaks the temp whenever
    /// the kernel raises (a user `valueOf` / getter / `toJSON` under
    /// the call).
    pub(crate) fn emit_throw_check_releasing(&mut self, eid: ExprId, op: &Operand) {
        let parked = self.park_owned_temp(eid, op);
        self.emit_throw_check(None);
        self.unpark_owned_temp(parked);
        self.release_owned_temp(eid, op);
    }

    /// Park a fresh-owned str-method argv operand for the post-call
    /// drain (`TempScratch::argv_owned`) AND for every throw edge
    /// between here and that drain.
    pub(crate) fn park_argv_owned(&mut self, op: Operand, ty: Type) {
        let tok = self.push_throw_temp(op, ty);
        self.temps.argv_owned.push((op, ty, tok));
    }
}
