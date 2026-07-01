//! Numeric-index normalization helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 379.
//!
//! Two SSA-emit helpers shared by every Array method that takes a
//! signed relative index (`copyWithin` / `slice` / `splice` / `fill`
//! / `indexOf` / `lastIndexOf` / etc.):
//!
//! - `clamp_i64_to_range(v, lo, hi)` — two-step condbr+slot+load
//!   clamp that mirrors the C runtime's clamp semantics for in-place
//!   Array bounds handling.
//! - `relative_to_len(v, len)` — JS-spec relative-index normalisation
//!   (`n < 0 ? n + len : n`, then `[0, len]` clamp). Coerces via
//!   `coerce_to_i64` at entry so fractional / NaN / ±∞ literals lower
//!   to a valid integer for the inline ICmp/Add/Store chain.
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Clamp an i64 SSA value to [lo, hi] via two `select` SSA-shape
    /// branches. Used by Array helpers that take user-provided indices
    /// (start / end / target) and need to match the C runtime's clamp
    /// semantics for the in-place case.
    pub(crate) fn clamp_i64_to_range(&mut self, v: Operand, lo: Operand, hi: Operand) -> Operand {
        // step 1: max(v, lo)
        let too_low = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v, lo),
            Type::Bool,
            None,
        );
        let lo_slot = self.alloca_in_entry(Type::I64, Some("__clamp_lo"));
        let lo_t = self.f.add_block();
        let lo_f = self.f.add_block();
        let lo_after = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_low),
                then_blk: lo_t,
                else_blk: lo_f,
            },
        );
        self.f
            .append_void(lo_t, InstKind::Store(lo, Operand::Value(lo_slot), 0));
        self.f.set_term(lo_t, Terminator::Br(lo_after));
        self.f
            .append_void(lo_f, InstKind::Store(v, Operand::Value(lo_slot), 0));
        self.f.set_term(lo_f, Terminator::Br(lo_after));
        self.cur_block = lo_after;
        let after_lo = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(lo_slot), 0),
            Type::I64,
            None,
        );
        // step 2: min(after_lo, hi)
        let too_high = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(after_lo), hi),
            Type::Bool,
            None,
        );
        let hi_slot = self.alloca_in_entry(Type::I64, Some("__clamp_hi"));
        let hi_t = self.f.add_block();
        let hi_f = self.f.add_block();
        let hi_after = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_high),
                then_blk: hi_t,
                else_blk: hi_f,
            },
        );
        self.f
            .append_void(hi_t, InstKind::Store(hi, Operand::Value(hi_slot), 0));
        self.f.set_term(hi_t, Terminator::Br(hi_after));
        self.f.append_void(
            hi_f,
            InstKind::Store(Operand::Value(after_lo), Operand::Value(hi_slot), 0),
        );
        self.f.set_term(hi_f, Terminator::Br(hi_after));
        self.cur_block = hi_after;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(hi_slot), 0),
            Type::I64,
            None,
        );
        Operand::Value(v)
    }

    /// V3-18 wedge — JS spec relative-index normalisation, per the
    /// pattern that copyWithin / slice / splice / etc. use:
    /// for an integer index `n` against an array length `len`:
    ///   if n < 0: n = max(len + n, 0)
    ///   if n >= len: n = len
    ///   else: n
    /// Emits the `n < 0 ? n + len : n` select via condbr+slot+load
    /// (no SSA Select instruction), then chains through
    /// clamp_i64_to_range for the [0, len] clamp.
    pub(crate) fn relative_to_len(&mut self, v: Operand, len: Operand) -> Operand {
        // ES ToIntegerOrInfinity at the call boundary — every callsite
        // (copyWithin / fill / slice / indexOf / lastIndexOf etc.)
        // feeds a numeric index that must be a signed integer for the
        // inline ICmp/Add/Store chain below. A fractional / NaN / ±∞
        // literal lowers to f64 and would panic backend GPR
        // materialization. coerce_to_i64 const-folds NaN→0 / ±∞→i64::
        // {MAX,MIN} and emits FpToSi for non-const f64 (no-op when
        // already i64).
        let v = self.coerce_to_i64(v);
        let is_neg = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v.clone(), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let plus_len = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, v.clone(), len.clone()),
            Type::I64,
            None,
        );
        let eff_slot = self.alloca_in_entry(Type::I64, Some("__rel_eff"));
        let neg_blk = self.f.add_block();
        let pos_blk = self.f.add_block();
        let join = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        self.f.append_void(
            neg_blk,
            InstKind::Store(Operand::Value(plus_len), Operand::Value(eff_slot), 0),
        );
        self.f.set_term(neg_blk, Terminator::Br(join));
        self.f
            .append_void(pos_blk, InstKind::Store(v, Operand::Value(eff_slot), 0));
        self.f.set_term(pos_blk, Terminator::Br(join));
        self.cur_block = join;
        let effective = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(eff_slot), 0),
            Type::I64,
            None,
        );
        self.clamp_i64_to_range(Operand::Value(effective), Operand::ConstI64(0), len)
    }
}
