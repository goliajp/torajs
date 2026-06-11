//! Unary operator lowering — `!` / `-` / `~` / `+` over every operand
//! type: Any-box arithmetic, Bool / null / string ToNumber coercion,
//! BigInt helpers, IEEE -0-preserving negation and ToInt32 bit-not
//! (extracted from `ssa_lower.rs`, file-size known-debt #1).

use crate::ast::{Expr, ExprId, UnaryOp};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    pub(crate) fn lower_unary(&mut self, op: UnaryOp, expr: ExprId) -> Operand {
        // M1.5 — `!a` lowers to `xor a, true`. Operand is bool,
        // result is bool. (BinOp::Xor on i1/i8 flips the low bit;
        // since bools only carry 0 or 1, this is logical not.)
        // M6.1 prereq — `-x` lowers to `0 - x`. f64 path emits
        // fsub from 0.0 (no SItoFP needed since both ops are
        // f64); i64 path emits sub from 0.
        //
        // Special case for `-NumberLit(0)`: the i64 narrowing
        // path collapses both `+0` and `-0` to `ConstI64(0)`,
        // losing IEEE 754 sign. We need `-0` to survive so
        // `Object.is(0, -0) === false` and `1 / -0 === -Infinity`
        // hold. Detect the AST shape `Unary(Neg, Number(0.0))`
        // and emit `ConstF64(-0.0)` directly, bypassing the
        // i64 path entirely.
        if matches!(op, crate::ast::UnaryOp::Neg)
            && let Expr::Number(n) = self.ast.get_expr(expr)
            && *n == 0.0
            && n.fract() == 0.0
        {
            return Operand::ConstF64(-0.0);
        }
        let v = self.lower_expr(expr);
        // P0.9 — Any operand on unary `-` / `+`: route through
        // any_arith helper. `-x` ≡ `0 - x` so we call any_arith
        // with op=Sub (0), LHS=ConstI64(0)+ANY_I64 tag, RHS=
        // unboxed-from-x. Result is fresh Any-box.
        // Step 7c: read tag/value via shim (was inline +8/+16).
        if matches!(op, crate::ast::UnaryOp::Neg | crate::ast::UnaryOp::Plus)
            && matches!(self.operand_ty(&v), Type::Any)
        {
            let r_tag = self.f.append_inst(
                self.cur_block,
                InstKind::Call(self.intrinsics.any_unbox_tag, vec![v.clone()]),
                Type::I64,
                None,
            );
            let r_value = self.f.append_inst(
                self.cur_block,
                InstKind::Call(self.intrinsics.any_unbox_value, vec![v]),
                Type::I64,
                None,
            );
            if matches!(op, crate::ast::UnaryOp::Neg) {
                // 0 - x via any_arith op=0
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_arith,
                        vec![
                            Operand::ConstI64(0), // op=Sub
                            Operand::ConstI64(2), // ANY_I64
                            Operand::ConstI64(0), // value 0
                            Operand::Value(r_tag),
                            Operand::Value(r_value),
                        ],
                    ),
                    Type::Any,
                    None,
                );
                return Operand::Value(r);
            } else {
                // Unary `+x` ≡ ToNumber(x) — call any_arith
                // op=Mul with LHS=1 to coerce to Number
                // without changing value.
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_arith,
                        vec![
                            Operand::ConstI64(1), // op=Mul
                            Operand::ConstI64(2), // ANY_I64
                            Operand::ConstI64(1), // value 1
                            Operand::Value(r_tag),
                            Operand::Value(r_value),
                        ],
                    ),
                    Type::Any,
                    None,
                );
                return Operand::Value(r);
            }
        }
        // V3-18 m1.f / m1.h.4 — coerce Bool / null before
        // unary `-`, `~`, `+`. For `-`, IEEE 754 -0 must
        // survive when the operand is the falsy 0
        // (-false / -null = -0.0 per bun), so we route via
        // f64 — the existing FSub-from-(-0.0) sign-preserving
        // path picks it up. For `~` and `+` integer is fine.
        let v = match op {
            crate::ast::UnaryOp::Neg => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstF64(0.0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    // bool → i64 → f64 chain so the sign-
                    // preserving FSub path picks it up.
                    let i = self.coerce_bool_to_i64(v);
                    self.coerce_to_f64(i)
                } else if matches!(self.operand_ty(&v), Type::Str | Type::Substr) {
                    // Unary-on-string wedge — `-s` per spec
                    // §13.5.5 calls ToNumber(s). Route through
                    // __torajs_str_to_number → f64; the F64
                    // Neg branch below picks it up and emits
                    // the sign-preserving FSub from -0.0.
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.str_to_number, vec![v]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
            crate::ast::UnaryOp::Plus => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstI64(0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    self.coerce_bool_to_i64(v)
                } else if matches!(self.operand_ty(&v), Type::Str | Type::Substr) {
                    // Unary-on-string wedge — `+s` per spec
                    // §13.5.4 calls ToNumber(s). Result is
                    // f64 so NaN can survive parse failures.
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.str_to_number, vec![v]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
            crate::ast::UnaryOp::BitNot => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstI64(0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    self.coerce_bool_to_i64(v)
                } else {
                    v
                }
            }
            _ => v,
        };
        match op {
            crate::ast::UnaryOp::Not => {
                // V3-18 m1.h.2 — coerce truthy first; the
                // existing xor-with-true path then flips.
                let v = self.coerce_to_bool(v);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, v, Operand::ConstBool(true)),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
            crate::ast::UnaryOp::Neg => {
                let v_ty = self.operand_ty(&v);
                match v_ty {
                    Type::BigInt => {
                        // T-25 — fresh +1 rc BigInt with
                        // sign flipped. Drop responsibility
                        // matches the rest of the BigInt
                        // arithmetic path (caller side).
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.bigint_neg, vec![v]),
                            Type::BigInt,
                            None,
                        );
                        return Operand::Value(r);
                    }
                    Type::F64 => {
                        // V3-18 m2.d follow-up — fold Neg on
                        // ConstF64 at lower-time. Otherwise
                        // `-Infinity` becomes a Value via FSub
                        // and downstream coerce_to_i64 sees an
                        // f64 Value (FpToSi → poison for
                        // non-finite, hangs / corrupts).
                        if let Operand::ConstF64(n) = v {
                            return Operand::ConstF64(-n);
                        }
                        // Use -0.0 (not +0.0) as the LHS so the
                        // ±0 sign is preserved: IEEE 754 gives
                        // (+0) - (+0) = +0 but (-0) - (+0) = -0,
                        // and (-0) - x = -x for all finite x.
                        // Required by `Object.is(0, -0) === false`
                        // and any other code that distinguishes
                        // signed zeros.
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::FSub, Operand::ConstF64(-0.0), v),
                            Type::F64,
                            None,
                        );
                        Operand::Value(r)
                    }
                    _ => {
                        // W3 C4 — fold Neg on ConstI64 at
                        // lower-time (mirror of the ConstF64
                        // fold above): a negative-literal
                        // operand must be visible to the Mul
                        // -0 float predicate (`x * -1`), which
                        // matches on Operand::ConstI64.
                        if let Operand::ConstI64(n) = v
                            && let Some(m) = n.checked_neg()
                        {
                            return Operand::ConstI64(m);
                        }
                        // W3 S8 (rfc 20260611-ann-width-unification
                        // §5.3) — `-x` on a runtime int mints -0
                        // when x == 0 (JS spec §13.5.5 unary minus),
                        // which i64 cannot represent, so a
                        // non-constant negation routes through the
                        // sign-preserving f64 FSub path. The
                        // frem_narrow fptosi sink recovers
                        // `sub(0, x)` where the -0 is unobservable
                        // (i64 truncation collapses it), same
                        // playbook as Mod / Mul.
                        if self.operand_ty(&v) == Type::I64 {
                            let vf = self.coerce_to_f64(v);
                            let r = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(SsaBinOp::FSub, Operand::ConstF64(-0.0), vf),
                                Type::F64,
                                None,
                            );
                            return Operand::Value(r);
                        }
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::Sub, Operand::ConstI64(0), v),
                            Type::I64,
                            None,
                        );
                        Operand::Value(r)
                    }
                }
            }
            crate::ast::UnaryOp::BitNot => {
                let v_ty = self.operand_ty(&v);
                if v_ty == Type::BigInt {
                    // V3-02 — BigInt `~x` ≡ `-x - 1n`. Routes
                    // through the bigint_not runtime helper
                    // (which uses the same identity).
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.bigint_not, vec![v]),
                        Type::BigInt,
                        None,
                    );
                    return Operand::Value(r);
                }
                // L3a-8 — `~x` is `ToInt32(x) ^ -1` per JS spec
                // §13.5.6: normalize to int32 first so
                // `~4294967295` gives 0 like v8 / jsc. The xor
                // of a sign-extended-32 value with -1 stays
                // sign-extended-32 — no post-normalization.
                let vi = self.coerce_f64_to_i64_for_bitwise(v);
                let v32 = self.emit_to_int32(vi);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, v32, Operand::ConstI64(-1)),
                    Type::I64,
                    None,
                );
                Operand::Value(r)
            }
            crate::ast::UnaryOp::Plus => {
                // V3-18 m1.h.4 — `+x` is ToNumber(x). For
                // already-numeric inputs we just pass through;
                // Bool/Null get coerced via the m1.f path
                // (already applied above for Neg/BitNot we
                // mirror here). The result type is Number
                // (i64 here, since the operand is now i64
                // after coerce).
                v
            }
        }
    }
}
