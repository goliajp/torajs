//! SSA-emit shorthand helpers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 382 — Path A.3-batch3.
//!
//! Four small one-purpose helpers that wrap common SSA emit shapes
//! reused across many lowering sites:
//!
//! - `bin(op, a, b, ty)` — emit `InstKind::BinOp(op, a, b)` and return
//!   its Operand::Value. Used everywhere arithmetic/logical BinOps are
//!   emitted mid-lowering.
//! - `cmp(pred, a, b)` — emit `InstKind::ICmp(pred, a, b)` with
//!   Type::Bool, return Operand::Value. Integer compares.
//! - `fcmp(pred, a, b)` — like `cmp` for FP.
//! - `raw_slot_arg(val)` — W4 raw-slot intrinsic argument adapter:
//!   `__torajs_arr_*` helpers take array slots as i64; an f64 value
//!   must cross as explicit bits (BitCastF64ToI64) to avoid the
//!   codegen-ambiguous FPR→i64-param path.
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ssa::{BinOp as SsaBinOp, FPred, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    pub(crate) fn bin(&mut self, op: SsaBinOp, a: Operand, b: Operand, ty: Type) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::BinOp(op, a, b), ty, None);
        Operand::Value(v)
    }

    pub(crate) fn cmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    pub(crate) fn fcmp(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    /// W4 — raw-slot intrinsic argument: array slots are 8 raw bytes
    /// and the `__torajs_arr_*` helpers take them as i64. An f64
    /// value must cross as explicit bits — passing an FPR value to an
    /// i64 param is codegen-ambiguous (the baseline tier reads the
    /// wrong register class; LLVM IR type-mismatches).
    pub(crate) fn raw_slot_arg(&mut self, val: Operand) -> Operand {
        if self.operand_ty(&val) != Type::F64 {
            return val;
        }
        match val {
            Operand::ConstF64(x) => Operand::ConstI64(x.to_bits() as i64),
            _ => Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::BitCastF64ToI64(val),
                Type::I64,
                None,
            )),
        }
    }
}
