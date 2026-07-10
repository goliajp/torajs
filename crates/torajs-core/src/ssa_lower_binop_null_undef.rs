//! ES §7.2.15 nullish strict-equality static folds pulled out of
//! [`crate::ssa_lower::lower_binop_inner`] as chunk-87 of the
//! decomp; rewritten type-based in chunk 612.
//!
//! Both `null` and `undefined` lower to a zero pointer at the SSA
//! layer — a raw ptr-cmp can't tell them apart, and an
//! Undefined/Null-typed BINDING is a Load (not `ConstPtrNull`), so
//! operand-shape checks can't either. Fold on the checker types
//! instead, via the `binop_{left,right}_{undef,null}_id` flags set
//! in `lower_binop_with_ids`:
//!
//! - Undefined vs Null (either order, literal or binding) → `===`
//!   false / `!==` true (spec §6.1.1/§6.1.2 distinct types).
//! - Undefined vs Undefined / Null vs Null → `===` true (both are
//!   single-value types).
//! - One nullish side vs a Str-typed value (RFC 20260707 chunk 2)
//!   → runtime identity cmp against the undefined sentinel cell
//!   (`=== undefined`) or NULL (`=== null`) — a Str slot can hold
//!   either nullish repr.
//! - Anything else → fall through (`None`) to the Any lane /
//!   same-family cmp paths.
//!
//! The pre-612 operand-shape version mistook the `undefined`
//! LITERAL (also ConstPtrNull) for `null`, folding
//! `y === undefined` to false for an Undefined-typed `y`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_substr::SUBSTR_UNDEF_CELL_SYM;

impl<'a> LowerCtx<'a> {
    /// RFC 20260710-optional-undefined-repr C1 — materialize the
    /// per-type undefined sentinel address for a Str/Substr slot
    /// (RFC 20260707 immortal cells; ADRP+ADD, zero calls). `None`
    /// for every other slot type — callers keep whatever value the
    /// undefined literal lowered to (NULL) until that type's
    /// sentinel lands (C2).
    pub(crate) fn str_undef_sentinel_for(&mut self, slot_ty: Type) -> Option<Operand> {
        let sym = match slot_ty {
            Type::Str => STR_UNDEF_CELL_SYM,
            Type::Substr => SUBSTR_UNDEF_CELL_SYM,
            // RFC 20260710 C2a — a fn-typed slot is Copy (no rc/drop
            // traffic), so the Str-shaped oddball serves as its
            // undefined repr directly: eq compares the address,
            // print/ToString consult the identity probe. Other
            // pointer slot types (Obj/Arr/Closure) keep NULL until
            // their inline-dec drop guards land (C2b).
            Type::FnSig(_) => STR_UNDEF_CELL_SYM,
            _ => return None,
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::GlobalRef(sym.to_string()),
            slot_ty,
            None,
        );
        Some(Operand::Value(v))
    }
}

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        return None;
    }
    let a_undef = ctx.binop_left_undef_id.is_some();
    let b_undef = ctx.binop_right_undef_id.is_some();
    let a_null = ctx.binop_left_null_id.is_some();
    let b_null = ctx.binop_right_null_id.is_some();
    let a_nullish = a_undef || a_null;
    let b_nullish = b_undef || b_null;
    if a_nullish && b_nullish {
        let eq = (a_undef && b_undef) || (a_null && b_null);
        let answer = match op {
            AstBinOp::Eq => eq,
            AstBinOp::Neq => !eq,
            _ => unreachable!(),
        };
        return Some(Operand::ConstBool(answer));
    }
    // RFC 20260707 chunk 2 (+ residual chunk) — one nullish side vs
    // a Str- or Substr-typed value. A Str slot can hold the Str
    // undefined sentinel cell (missed exec/match capture) or NULL
    // (JS null); a Substr slot can hold the Substr-shaped sentinel
    // (string index OOB read): `x === undefined` compares against
    // the matching sentinel ADDRESS, `x === null` against NULL.
    if a_nullish ^ b_nullish {
        let (val, nullish_is_undef) = if a_nullish {
            (b, a_undef)
        } else {
            (a, b_undef)
        };
        let val_ty = ctx.operand_ty(&val);
        // RFC 20260708-typed-arr-oob-read chunk 2 — an undefable F64
        // side vs the `undefined` literal compares the bits against
        // the sentinel pattern. Plain F64s (and `=== null`) keep the
        // cross-type false fold downstream.
        if val_ty == Type::F64 {
            let undefable = if a_nullish {
                ctx.binop_right_f64_undefable
            } else {
                ctx.binop_left_f64_undefable
            };
            if !undefable || !nullish_is_undef {
                return None;
            }
            let bits = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BitCastF64ToI64(val),
                Type::I64,
                None,
            );
            let cmp = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(
                    IPred::Eq,
                    Operand::Value(bits),
                    Operand::ConstI64(
                        crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS as i64,
                    ),
                ),
                Type::Bool,
                None,
            );
            if matches!(op, AstBinOp::Neq) {
                let r = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, Operand::Value(cmp), Operand::ConstBool(true)),
                    Type::Bool,
                    None,
                );
                return Some(Operand::Value(r));
            }
            return Some(Operand::Value(cmp));
        }
        // RFC 20260710 C2a — fn-typed slots join the sentinel-capable
        // set (their undefined repr is the Str-shaped oddball).
        if !matches!(val_ty, Type::Str | Type::Substr | Type::FnSig(_)) {
            return None;
        }
        let rhs = if nullish_is_undef {
            ctx.str_undef_sentinel_for(val_ty)
                .expect("gate admits only sentinel-capable slot types")
        } else {
            Operand::ConstPtrNull
        };
        let cmp = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, val, rhs),
            Type::Bool,
            None,
        );
        if matches!(op, AstBinOp::Neq) {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, Operand::Value(cmp), Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(r));
        }
        return Some(Operand::Value(cmp));
    }
    None
}
