//! Keeps the F64 `undefined` sentinel out of arithmetic results.
//!
//! The sentinel
//! ([`crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS`])
//! is a quiet NaN with a payload chosen so that nothing but a
//! genuine `undefined` wears it. That claim used to rest on "no
//! arithmetic produces these bits", which is false on AArch64:
//! FPCR.DN=0 means a binary op with one NaN operand hands back
//! *that operand's payload*, so `xs[oob] * 2` is bit-for-bit the
//! sentinel. Measured, not assumed — `s + 1.0`, `s * 1.0`,
//! `s - s` all return `0x7ffc_0000_0000_000a`.
//!
//! On its own that is invisible: every consumer gates statically
//! on [`crate::ssa_lower_nullable_guard::is_undef_f64_source`], and
//! an arithmetic expression is not one. It becomes visible the
//! moment the result is **stored into a container**, because
//! reading it back out is an index or field read — a shape the
//! static gate does arm on. `const xs = [zs[oob] + 1]; xs[0]`
//! then answered `undefined` where JS says `NaN`.
//!
//! The fix is the spec step that was missing rather than a guard
//! around the symptom: ToNumber(undefined) is NaN (§7.1.4), so a
//! sentinel must become a plain NaN *on the way into* a numeric
//! operation. That establishes the invariant the consumers rely
//! on — **no arithmetic result ever carries the sentinel payload**,
//! hence sentinel bits in an F64 location mean a genuine
//! `undefined`.
//!
//! Cleaning the operand rather than the result is what makes it
//! complete. A result-side check only catches results that are
//! bit-equal to the sentinel, and negation is not: `-sentinel`
//! flips the sign bit into a pattern the compare misses, and a
//! second negation puts it back. `-(-xs[oob])` walked straight
//! through a result-side check and answered `undefined`. An
//! operand that is never the sentinel cannot produce one.
//!
//! Cost is confined to operations the static gate arms — ones with
//! an out-of-range-read-shaped operand — so numeric loops emit
//! exactly what they emitted before.
//!
//! Shaped as a branch through a merge slot rather than a `Select`,
//! because `Select` is the egraph's own output form and reaching
//! the elaborator with one lowered by hand is an `unreachable!`.
//! The sibling sentinel branches (`box_f64_or_undef`) spell it the
//! same way.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS;

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
