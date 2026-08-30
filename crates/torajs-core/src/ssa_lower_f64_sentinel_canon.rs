//! Keeps the F64 `undefined` sentinel out of a Math kernel result.
//!
//! The sentinel
//! ([`crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS`])
//! is a quiet NaN with a payload chosen so that nothing but a
//! genuine `undefined` wears it, and the program entry selects
//! FPCR.DN so that the machine cannot forge it: every FP operation
//! returning a NaN returns the default one, whatever its operands
//! carried (`torajs-cli::cmd_build_synthesize::FPCR_DN_BIT`).
//!
//! Two instructions sit outside that guarantee, and ARM ARM
//! C6.2.104 / C6.2.106 say why — FNEG and FABS are sign-bit writes,
//! not arithmetic, so they hand a payload straight through. `-` is
//! lowered as an FSub here and is covered; `Math.abs` is the one
//! kernel that is literally FABS, and it returns the sentinel
//! unchanged. `Math.abs(xs[oob])` would then read back as
//! `undefined` where the spec says `NaN`, so the argument is
//! cleaned before the kernel sees it.
//!
//! Cleaning the operand rather than the result is what makes that
//! complete: a result-side check only catches results bit-equal to
//! the sentinel, and the sign ops are exactly the ones that can
//! leave and re-enter that pattern — `-sentinel` misses the compare
//! and a second negation restores it. An operand that is never the
//! sentinel cannot produce one.
//!
//! Cost lands only where [`crate::ssa_lower_undef_f64_source::
//! is_undef_f64_source`] arms, so a Math call on a value that
//! cannot be a sentinel emits what it always did.
//!
//! Shaped as a branch through a merge slot rather than a `Select`,
//! because `Select` is the egraph's own output form and reaching
//! the elaborator with one lowered by hand is an `unreachable!`.
//! The sibling sentinel branches (`box_f64_or_undef`) spell it the
//! same way.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS;

/// The hardware default quiet NaN — what AArch64 produces for
/// `0.0 / 0.0` and what every payload-free NaN in the engine looks
/// like. Written as bits and bitcast rather than `f64::NAN` so the
/// pattern survives any float-constant handling on the way down.
const PLAIN_QNAN_BITS: i64 = 0x7FF8_0000_0000_0000u64 as i64;

impl LowerCtx<'_> {
    /// Replace the sentinel bit pattern with a plain quiet NaN,
    /// leaving every other value — NaN or not — untouched.
    pub(crate) fn canon_f64_away_from_sentinel(&mut self, v: Operand) -> Operand {
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BitCastF64ToI64(v.clone()),
            Type::I64,
            None,
        );
        let is_sentinel = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(bits),
                Operand::ConstI64(F64_UNDEF_SENTINEL_BITS as i64),
            ),
            Type::Bool,
            None,
        );
        let plain_blk = self.f.add_block();
        let keep_blk = self.f.add_block();
        let merge = self.f.add_block();
        let slot = self.alloca_in_entry(Type::F64, Some("__f64canon"));
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_sentinel),
                then_blk: plain_blk,
                else_blk: keep_blk,
            },
        );
        self.cur_block = plain_blk;
        let plain = self.f.append_inst(
            self.cur_block,
            InstKind::BitCastI64ToF64(Operand::ConstI64(PLAIN_QNAN_BITS)),
            Type::F64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(plain), Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = keep_blk;
        self.f
            .append_void(self.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let out = self.f.append_inst(
            merge,
            InstKind::Load(Type::F64, Operand::Value(slot), 0),
            Type::F64,
            None,
        );
        Operand::Value(out)
    }
}
