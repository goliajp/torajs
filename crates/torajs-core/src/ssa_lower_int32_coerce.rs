//! Int32-coerce family for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 387 — Path A.3-batch8.
//!
//! Six methods forming the JS-spec §7.1.6 ToInt32 / §7.1.7 ToUint32
//! lowering family — used by every path that has to feed a number
//! into a Rust runtime helper that expects `i64` or reproduce
//! v8/jsc-compatible bitwise semantics at 32-bit width:
//!
//! - `coerce_bool_to_i64(op)` — Bool → i64 slot (uniform helper ABI).
//! - `coerce_to_i64(op)` — F64/Bool/I64 → i64 with spec ToInteger
//!   folding on non-finite constants (NaN→0, ±∞→i64::{MIN,MAX}).
//! - `coerce_f64_to_i64_for_bitwise(op)` — bitwise-path variant of
//!   the above, treating any non-finite constant as 0 per spec.
//! - `emit_to_int32(op)` — normalize an i64 to sign-extended int32
//!   via `shl 32; ashr 32` (LLVM folds to a single `sxtw` on arm64).
//! - `emit_shift_count(op)` — spec §13.9 shift-count masking
//!   (`op & 31`); private helper only used by `lower_bitwise_int32`.
//! - `lower_bitwise_int32(op, a, b)` — shared int32-semantics
//!   lowering for the six bitwise/shift operators (&|^, `<<`, `>>`,
//!   `>>>`) — both the all-i64 and the mixed-f64 binop paths land
//!   here.
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Widen a Bool / i1 operand to the i64-shaped slot used by uniform
    /// runtime helpers (array push, object field store, hashmap value,
    /// throw_value). Constants are rewritten in place; SSA values go
    /// through an explicit `ZExtBoolToI64` instruction. No-op when the
    /// operand is already i64-shaped.
    pub(crate) fn coerce_bool_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::Bool => match op {
                Operand::ConstBool(b) => Operand::ConstI64(if b { 1 } else { 0 }),
                Operand::Value(_) => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(op),
                        Type::I64,
                        None,
                    );
                    Operand::Value(v)
                }
                _ => op,
            },
            _ => op,
        }
    }

    /// Truncate an f64 operand to i64. Mirrors `coerce_to_f64` for the
    /// reverse direction — used at call sites whose runtime intrinsic
    /// expects an integer parameter (Math.imul, Math.clz32) but the
    /// caller may have passed a float literal or a Math.* result.
    /// Constants fold in place; value operands emit `InstKind::FpToSi`.
    pub(crate) fn coerce_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                // V3-18 m2.d follow-up — JS spec ToInteger:
                //   NaN  → 0
                //   ±Inf → ±Infinity (here represented by i64::MAX /
                //          i64::MIN — preserves sign through any
                //          downstream "from >= len" / "from + len"
                //          clamp logic).
                // Without this, FpToSi on non-finite is poison and
                // downstream loops get stuck or read garbage.
                Operand::ConstF64(n) => {
                    if n.is_nan() {
                        Operand::ConstI64(0)
                    } else if n == f64::INFINITY {
                        Operand::ConstI64(i64::MAX)
                    } else if n == f64::NEG_INFINITY {
                        Operand::ConstI64(i64::MIN)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64"),
        }
    }

    /// V3-18 m1.h.40 — JS spec §7.1.6 ToInt32 for the bitwise-on-Number
    /// path. Constants fold at compile time (NaN / ±Inf → 0; finite
    /// truncates towards zero); SSA values use FpToSi (matching the
    /// finite-in-i32-range behavior LLVM gives, with NaN / OOB
    /// landing as poison — same as v8 / jsc in practice for the
    /// integer bitwise idioms we exercise).
    pub(crate) fn coerce_f64_to_i64_for_bitwise(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                Operand::ConstF64(n) => {
                    if !n.is_finite() {
                        Operand::ConstI64(0)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64 for bitwise"),
        }
    }

    /// JS spec §7.1.6 ToInt32 normalization on tora's i64 value model:
    /// sign-extend the operand's low 32 bits (`shl 32` + `ashr 32` —
    /// LLVM folds the pair to a single sxtw on arm64). Bitwise/shift
    /// operators apply this to each operand (and to the Shl result,
    /// which can carry past bit 31) so `1 << 31` wraps negative and
    /// `4294967296 | 0` truncates to 0 exactly like v8 / jsc.
    pub(crate) fn emit_to_int32(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n as i32 as i64),
            _ => {
                let hi = self.bin(SsaBinOp::Shl, op, Operand::ConstI64(32), Type::I64);
                self.bin(SsaBinOp::AShr, hi, Operand::ConstI64(32), Type::I64)
            }
        }
    }

    /// JS spec §13.9 — shift counts take `ToUint32(b) & 31`, which is
    /// exactly the low 5 bits of the i64 operand (two's complement
    /// agrees for negative counts: `-3 & 31 == 29 == ToUint32(-3) & 31`).
    fn emit_shift_count(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n & 31),
            _ => self.bin(SsaBinOp::And, op, Operand::ConstI64(31), Type::I64),
        }
    }

    /// Shared int32-semantics lowering for the six bitwise/shift
    /// operators on Number (the all-i64 and the mixed-f64 binop paths
    /// both land here; f64 operands first truncate via
    /// `coerce_f64_to_i64_for_bitwise`). Per JS spec §13.9 / §13.12 each
    /// operand is ToInt32-normalized (ToUint32 for `>>>`), the op runs
    /// at 32-bit width, and the result sign-extends back — emitted as
    /// explicit i64 SSA insts so every downstream pass (egraph
    /// const-fold included) stays a plain i64-semantics transform.
    pub(crate) fn lower_bitwise_int32(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        let ai = self.coerce_f64_to_i64_for_bitwise(a);
        let bi = self.coerce_f64_to_i64_for_bitwise(b);
        match op {
            // And/Or/Xor of two sign-extended-32 values is itself
            // sign-extended-32 — no post-normalization needed.
            AstBinOp::BitAnd => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::And, a32, b32, Type::I64)
            }
            AstBinOp::BitOr => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Or, a32, b32, Type::I64)
            }
            AstBinOp::BitXor => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Xor, a32, b32, Type::I64)
            }
            // `a << b` can carry past bit 31 — re-normalize the result.
            AstBinOp::Shl => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                let r = self.bin(SsaBinOp::Shl, a32, cnt, Type::I64);
                self.emit_to_int32(r)
            }
            // ashr of a sign-extended-32 value stays sign-extended-32.
            AstBinOp::Shr => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::AShr, a32, cnt, Type::I64)
            }
            // `>>>` is ToUint32: zero-extend the low 32 bits, then
            // logical-shift — result is in [0, 2^32), non-negative.
            AstBinOp::UShr => {
                let mask32 = self.bin(SsaBinOp::And, ai, Operand::ConstI64(0xFFFF_FFFF), Type::I64);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::LShr, mask32, cnt, Type::I64)
            }
            other => unreachable!("lower_bitwise_int32: non-bitwise op {other:?}"),
        }
    }
}
