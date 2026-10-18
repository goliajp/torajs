//! The fill loop's reserve bound when the loop bound is an f64 — a
//! size that arrived at run time (`parseInt(process.argv[2], 10)`)
//! rather than as a literal.
//!
//! The loop header is untouched: it still compares `i` against the
//! f64, so the trip count is whatever it was. What this computes is
//! the capacity those trips need, as the i64 the install flow takes:
//!
//! | bound `n`            | trips        | reserved |
//! |----------------------|--------------|----------|
//! | NaN, `n <= 0`        | 0            | 0        |
//! | `0 < n <= 2^53`      | `ceil(n)`    | `ceil(n)`|
//! | `n > 2^53`, infinity | never ends   | `2^53`   |
//!
//! The last row reserves more than the address space holds, so the
//! allocation fails before the first store, where the checked lane
//! fails once memory runs out.

use crate::ssa::{BinOp as SsaBinOp, FPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

const TWO_POW_53: i64 = 1 << 53;

impl LowerCtx<'_> {
    /// Branch- and select-free: `Select` only exists after the egraph
    /// pass.
    pub(crate) fn emit_f64_trip_count(&mut self, n: Operand) -> Operand {
        let positive = self.emit_fcmp_i64(FPred::Ogt, n, Operand::ConstF64(0.0));
        let in_window = self.emit_fcmp_i64(FPred::Ole, n, Operand::ConstF64(TWO_POW_53 as f64));
        let truncated = self.emit_trip_i64(InstKind::FpToSi(n));
        let back = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::SiToFp(truncated),
            Type::F64,
            None,
        ));
        let round_up = self.emit_fcmp_i64(FPred::Olt, back, n);
        let ceil = self.emit_trip_i64(InstKind::BinOp(SsaBinOp::Add, truncated, round_up));
        // a wrapped `ceil` (infinity, NaN) is multiplied by zero
        let exact = self.emit_trip_i64(InstKind::BinOp(SsaBinOp::Mul, ceil, positive));
        let exact = self.emit_trip_i64(InstKind::BinOp(SsaBinOp::Mul, exact, in_window));
        let beyond = self.emit_trip_i64(InstKind::BinOp(
            SsaBinOp::Sub,
            Operand::ConstI64(1),
            in_window,
        ));
        let saturated = self.emit_trip_i64(InstKind::BinOp(
            SsaBinOp::Mul,
            Operand::ConstI64(TWO_POW_53),
            positive,
        ));
        let saturated = self.emit_trip_i64(InstKind::BinOp(SsaBinOp::Mul, saturated, beyond));
        self.emit_trip_i64(InstKind::BinOp(SsaBinOp::Add, exact, saturated))
    }

    fn emit_fcmp_i64(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let flag = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
        self.emit_trip_i64(InstKind::ZExtBoolToI64(Operand::Value(flag)))
    }

    fn emit_trip_i64(&mut self, kind: InstKind) -> Operand {
        Operand::Value(self.f.append_inst(self.cur_block, kind, Type::I64, None))
    }
}
