//! Number-coerce pair for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 388 — Path A.3-batch9.
//!
//! Two methods forming the "coerce to concrete numeric" sink family
//! (mirror to the int32-coerce family in `ssa_lower_int32_coerce.rs`
//! — that side handles Number → int32 semantics; this side handles
//! I64/Any → F64):
//!
//! - `coerce_to_f64(op)` — promote an i64 operand to f64. Constants
//!   rewrite in place; SSA values emit `InstKind::SiToFp`.
//! - `coerce_any_to_number(op, target)` — P7.2b: coerce a `Type::Any`
//!   operand to a concrete numeric via the JS spec §7.1.4 ToNumber
//!   sink (`__torajs_any_to_number` runtime helper → F64), then narrow
//!   to `target` (F64 as-is, or I64 via the existing F64→i64
//!   ToInteger path). Single place for the Any→number sink so
//!   `Stmt::Return` / `Assign` / call-arg coercion can't drift apart.
//!
//! Method bodies preserved byte-for-byte; the sibling reaches LowerCtx
//! fields via `impl<'a> super::LowerCtx<'a>`, so call sites (~50 across
//! sibling modules — call_terminal / binop_inner_f64 / stmt_return /
//! array / arr_push / assign_member / …) need zero edits.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    pub(crate) fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::SiToFp(op), Type::F64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to f64"),
        }
    }

    /// P7.2b — coerce an Any operand to a concrete numeric: JS spec
    /// §7.1.4 ToNumber via the one `__torajs_any_to_number` runtime
    /// helper, then narrowed to `target` (F64 as-is, or I64 via the
    /// existing F64→i64 ToInteger path). Single place for the
    /// Any→number sink so Stmt::Return and Assign can't drift apart
    /// (mirrors coerce_to_bool's `Type::Any => any_to_bool`
    /// precedent). Caller guarantees `operand_ty(op) == Type::Any`
    /// and `target` ∈ {I64, F64}.
    pub(crate) fn coerce_any_to_number(&mut self, op: Operand, target: Type) -> Operand {
        let num = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        ));
        if target == Type::F64 {
            num
        } else {
            self.coerce_to_i64(num)
        }
    }

    /// Any → BigInt at a call boundary (§7.1.13 ToBigInt via the
    /// any-lane kernel; RFC 20260720 刀 5b-2). The result is a fresh
    /// OWNED BigInt the caller must release after the consuming call;
    /// a coercion reject records a pending throw (TypeError /
    /// SyntaxError) and the emitted throw check unwinds before the
    /// NULL sentinel is ever read.
    pub(crate) fn coerce_any_to_bigint(&mut self, op: Operand) -> Operand {
        let b = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_bigint, vec![op]),
            Type::BigInt,
            None,
        ));
        self.emit_throw_check(None);
        b
    }
}
