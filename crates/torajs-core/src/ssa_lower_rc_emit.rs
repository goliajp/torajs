//! Refcount inc / dec / drop emit helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 378.
//!
//! Four ARC-emit helpers that are the single retrofit surface for the
//! future biased ARC (owner-thread fast path + share transition +
//! atomic slow path,see `.claude/vision.md` 三-1 节 +
//! `rules/torajs-design-principles.md` §6.2):
//!
//! - `emit_rc_inc(op)`         — inc in current block
//! - `emit_rc_inc_in(blk, op)` — inc in an explicit block (branch tails)
//! - `emit_rc_dec_inline(hdr)` — inline Load-Sub-Store dec, returns new rc
//! - `emit_drop_value(v, ty)`  — thin proxy into `ssa_lower_emit_drop_value`
//!
//! **HARD RULE (§6.2):** all refcount inc/dec emit goes through these
//! helpers or the typed drop intrinsics; direct
//! `InstKind::Call(intrinsics.rc_inc/rc_dec, ...)` in lowering code is
//! a §6 violation. Method bodies are byte-for-byte preserved from the
//! source; sibling reaches LowerCtx fields via
//! `impl<'a> super::LowerCtx<'a>`, so call sites need zero edits.

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Emit a refcount inc on `op`. Today expands to a single
    /// `Call(intrinsics.rc_inc, vec![op])` — semantically and
    /// instruction-wise equivalent to a direct emit. This helper
    /// is the single retrofit point for the future biased ARC
    /// (owner-thread fast path 0 atomic 增量 + share transition +
    /// atomic 慢路径,详见 `.claude/vision.md` 三-1 节 +
    /// `rules/torajs-design-principles.md` §6.2)。
    ///
    /// **HARD RULE (§6.2):** all refcount inc emit goes through
    /// this helper. Direct `InstKind::Call(intrinsics.rc_inc, ...)`
    /// in lowering code is a §6 violation.
    pub(crate) fn emit_rc_inc(&mut self, op: Operand) {
        let block = self.cur_block;
        self.emit_rc_inc_in(block, op);
    }

    /// Same as [`emit_rc_inc`] but emits into an explicit `block`
    /// instead of `self.cur_block`. Used by control-flow shapes that
    /// build a fresh `then_end` / `else_blk` and need to inc in a
    /// branch tail (e.g. Nullish-coalescing `??`).
    pub(crate) fn emit_rc_inc_in(&mut self, block: BlockId, op: Operand) {
        self.f
            .append_void(block, InstKind::Call(self.intrinsics.rc_inc, vec![op]));
    }

    /// Type-aware owned-result inc (RFC 20260705 owned-result
    /// invariant): builtin lowerings that answer a borrowed value
    /// (receiver identity / pass-through arg) call this so the
    /// result carries its own ref. `Type::Any` routes through the
    /// NaN-box-gated `any_box_rc_inc` runtime helper (immediates
    /// are no-ops); other refcounted types take the plain header
    /// inc; Copy types need none.
    pub(crate) fn emit_owned_result_inc(&mut self, op: Operand, ty: Type) {
        if ty == Type::Any {
            self.f.append_void(
                self.cur_block,
                InstKind::Call(self.intrinsics.any_box_rc_inc, vec![op]),
            );
        } else if ty.is_refcounted() {
            self.emit_rc_inc(op);
        }
    }

    /// Emit an inline refcount dec on the heap-header pointer `hdr`.
    /// Returns the new refcount value (Type::I32) so the caller can
    /// `ICmp(Eq, _, ConstI32(0))` to dispatch to drop. Mirrors the
    /// existing Bacon-Rajan inline shape: Load i32 @ offset 0 →
    /// `Sub 1` → Store back.
    ///
    /// Future biased ARC swap-point: this helper expands to an
    /// owner-thread check + atomic_rmw fetch_sub for shared objects.
    /// Today equivalent to the raw Load-Sub-Store sequence.
    ///
    /// **HARD RULE (§6.2):** all refcount dec emit goes through
    /// this helper or through the typed drop helpers
    /// (`emit_drop_value` / `intrinsics.{str_drop, arr_drop,
    /// substr_drop, value_drop_heap}`).
    pub(crate) fn emit_rc_dec_inline(&mut self, hdr: Operand) -> Operand {
        let rc_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I32, hdr.clone(), 0),
            Type::I32,
            None,
        );
        let rc_new = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc_now), Operand::ConstI32(1)),
            Type::I32,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(rc_new), hdr, 0),
        );
        Operand::Value(rc_new)
    }

    pub(crate) fn emit_drop_value(&mut self, val: Operand, ty: Type) {
        crate::ssa_lower_emit_drop_value::emit(self, val, ty)
    }

    /// RFC 20260705 owned-result invariant — true when the
    /// expression's lowered result is an owned temp its consumer
    /// must release: Call/New always answer an owned value (fresh
    /// alloc, +1 return retain, or the borrow sites' owned-result
    /// inc); BinOp results are fresh too (str concat) except the
    /// short-circuit LAnd/LOr which answer an operand borrow;
    /// Closure literals mint a fresh env block. An `as` cast is a
    /// pass-through at the value layer (lower_as_cast answers the
    /// inner operand for the heap-typed cases), so ownership follows
    /// the inner expression — without the recursion `f() as K`
    /// looked like a borrow and every consumer's release turned
    /// no-op (probe l16d: `(wr.deref() as K).x` churn kept the
    /// target alive after its only strong ref died). Every other
    /// expression shape (Ident / Member / Index / literal) answers
    /// a borrow — except the minted-closure Ident caught by
    /// [`Self::release_owned_temp`]'s operand-type check.
    pub(crate) fn expr_owned_shape(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Call { .. } | Expr::New { .. } | Expr::Closure { .. } => true,
            Expr::BinOp { op, .. } => !matches!(op, AstBinOp::LAnd | AstBinOp::LOr),
            Expr::As { expr, .. } => self.expr_owned_shape(*expr),
            _ => false,
        }
    }

    /// True when `eid` is a lifted-arrow Ident (`__closure_<N>`
    /// shape after lift_arrow_fns, not a local binding) whose
    /// lowering minted a fresh closure env — the fn-intro path
    /// answers `Type::Closure` with a heap env block the consumer
    /// owns. A user `let`-bound closure Ident is a local slot
    /// borrow and stays off this predicate.
    fn expr_minted_closure(&self, eid: ExprId, op: &Operand) -> bool {
        matches!(
            self.ast.get_expr(eid),
            Expr::Ident(n) if !self.locals.contains_key(n)
        ) && matches!(self.operand_ty(op), Type::Closure(_))
    }

    /// Release the operand lowered from `eid` iff it is an
    /// owned-shape temp (see [`Self::expr_owned_shape`]) of a
    /// non-Copy type, or a minted-closure Ident (see
    /// [`Self::expr_minted_closure`]). Consumers that borrow a
    /// sub-expression result (method-call receivers, call
    /// arguments, callback slots) call this after the consuming
    /// instruction so nested-call temps (`f(g())`,
    /// `a.reverse().sort()`) and per-call closure envs
    /// (`xs.map(x => ...)`) don't leak their ref.
    pub(crate) fn release_owned_temp(&mut self, eid: ExprId, op: &Operand) {
        if !self.expr_owned_shape(eid) && !self.expr_minted_closure(eid, op) {
            return;
        }
        let ty = self.operand_ty(op);
        if ty.is_copy() {
            return;
        }
        self.emit_drop_value(op.clone(), ty);
    }

    /// True when the expression's lowered value is an owned temp
    /// whose reference transfers straight into an owning consumer
    /// (an `any` box, an `Arr<Any>` slot) with no +1 needed:
    /// Call / New / BinOp / Closure per [`Self::expr_owned_shape`],
    /// string indexing (fresh Substr view, chunk 561), and string
    /// literals (static cells — rc traffic is a no-op, skip the
    /// inc). Borrow shapes (Ident / Member / container Index)
    /// answer false; the consumer takes +1 so the source binding
    /// keeps its own stake (chunks 563/565).
    pub(crate) fn expr_transfers_ownership(&self, eid: ExprId) -> bool {
        if self.expr_owned_shape(eid) {
            return true;
        }
        match self.ast.get_expr(eid) {
            Expr::Index { obj, .. } => {
                matches!(self.expr_types.get(obj), Some(crate::check::Type::String))
            }
            Expr::String(_) => true,
            _ => false,
        }
    }
}
