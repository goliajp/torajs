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
        let block = self.cur_block;
        self.emit_owned_result_inc_in(block, op, ty);
    }

    /// Same as [`emit_owned_result_inc`] but emits into an explicit
    /// `block`. Used by control-flow joins (Ternary / Nullish) that
    /// unify a borrow branch to owned in that branch's tail block.
    pub(crate) fn emit_owned_result_inc_in(&mut self, block: BlockId, op: Operand, ty: Type) {
        if ty == Type::Any {
            self.f.append_void(
                block,
                InstKind::Call(self.intrinsics.any_box_rc_inc, vec![op]),
            );
        } else if ty.is_refcounted() {
            self.emit_rc_inc_in(block, op);
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
            // Chunk 721 — OptCall answers a fresh owned Any box on
            // both arms (hit = the call's owned result, miss = an
            // undef immediate whose drop no-ops), per the chunk-705
            // contract; it was missing from this predicate, so a
            // discarded / printed `f?.()` result had no release site.
            Expr::Call { .. } | Expr::New { .. } | Expr::Closure { .. } | Expr::OptCall { .. } => {
                true
            }
            // Rotation 325 — a short-circuit `&&` / `||` join is a
            // borrow UNLESS its lowering unified the arms to owned
            // (an owned any-member read rode into the slot — see
            // lower_logical_and); those joins record their eid on
            // the same track the Ternary / Nullish joins use.
            Expr::BinOp { op, .. } if matches!(op, AstBinOp::LAnd | AstBinOp::LOr) => {
                self.owned_member_reads.contains(&eid)
            }
            Expr::BinOp { .. } => true,
            Expr::As { expr, .. } => self.expr_owned_shape(*expr),
            // Chunk 640 — array / object literals mint a fresh heap
            // block (every lower_array / ObjectLit lane answers
            // rc=1). Off this predicate, a literal consumed as a
            // call argument had no release site (`g([1,2,3])` churn
            // leaked every block — probe l22 44.8MB / l22b 25.5MB
            // vs 6.4MB flat) and owning consumers (any-box, push,
            // field store) added their own inc on top of the
            // stranded +1.
            Expr::Array(_) | Expr::ObjectLit { .. } => true,
            // Rotation 542 — a BigInt literal is a fresh heap block
            // exactly like the two above: `lower_bigint` calls
            // `bigint_from_decimal` / `_from_hex`, which answer rc=1.
            // Off this predicate it had no release site anywhere and
            // owning consumers added their own inc on top of the
            // stranded +1 — the chunk-640 failure, one literal shape
            // later. 200k churn against the array literal as control,
            // same three positions: `take(2n)` 14.3MB vs `take([1,2])`
            // 1.49MB, `const a: any = 2n` 14.4MB vs 1.56MB,
            // `xs.push(2n)` 14.4MB, against 1.44MB flat.
            Expr::BigInt { .. } => true,
            // An update expression over an `any` slot answers the
            // runtime step's own coerced value, owned. The typed lanes
            // answer a bare number, which `release_owned_temp`'s copy
            // check drops out of before reaching a release site.
            Expr::PostIncr { .. } => true,
            // Chunk 637 — a Member read is normally a receiver
            // borrow, but `ssa_lower_member::lower` detaches the
            // result (owned inc) when the receiver was itself an
            // owned temp; those eids are recorded and answer owned.
            // Chunk 717 — the any-member read lanes (Member on any,
            // `o["k"]` literal-key Index, OptChain/OptIndex hit
            // paths, Closure expando reads) answer owned on every
            // arm and record their eid the same way; unrecorded
            // Index/OptChain/OptIndex reads stay borrows.
            Expr::Member { .. }
            | Expr::Index { .. }
            | Expr::OptChain { .. }
            | Expr::OptIndex { .. } => self.owned_member_reads.contains(&eid),
            // Chunk 722 — a Ternary / Nullish join whose lowering
            // unified the branches to owned (any branch was an
            // owned shape; the borrow branch got a tail-block inc)
            // records its eid the same way; joins over pure borrows
            // (`cond ? a : b` over idents) stay borrows with zero
            // rc traffic (probe p722a ternary-discard 15.3MB /
            // p722d-e nullish 15.3MB vs 6.2MB flat).
            Expr::Ternary { .. } | Expr::Nullish { .. } => self.owned_member_reads.contains(&eid),
            // Rotation 323 — an Ident-target assignment answers the
            // stored value with a consumer stake minted at the store
            // (`ssa_lower_assign_ident::mint_consumer_stake`) and
            // records its eid; without that stake a kept result
            // (`b = (a = [1,2,3])`) had two releases against one
            // reference — underflow on freed memory, surfacing as a
            // segfault in the exit-drain cycle walk. Member / Index
            // targets don't mint (their lanes aren't a uniform slot
            // borrow) and stay borrows.
            Expr::Assign { .. } => self.owned_member_reads.contains(&eid),
            _ => false,
        }
    }

    /// True when `eid` is a lifted-arrow Ident (`__closure_<N>`
    /// shape after lift_arrow_fns, not a local binding) whose
    /// lowering minted a fresh closure env — the fn-intro path
    /// answers `Type::Closure` with a heap env block the consumer
    /// owns. A user `let`-bound closure Ident is a local slot
    /// borrow and stays off this predicate.
    pub(crate) fn expr_minted_closure(&self, eid: ExprId, op: &Operand) -> bool {
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
        // Peel value-transparent `As` wrappers before the two arms
        // below. `expr_owned_shape` above already recurses through
        // them, but these two match the raw node, so `s[0] as any`
        // missed the fresh-Substr arm and answered "borrow" -- the
        // consumer then took a +1 on a value nobody else holds and
        // nobody releases. This is the LEAK direction of the same
        // blind spot the four use-after-frees came from (rotation
        // 385): 300k of `const t: any = s[0] as any` peaked at
        // 16.07 MB RSS against 6.46 MB for the uncast `s[0]`.
        let mut eid = eid;
        while let Expr::As { expr, .. } = self.ast.get_expr(eid) {
            eid = *expr;
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
