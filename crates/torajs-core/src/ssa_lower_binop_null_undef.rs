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
pub(crate) use undef_cell::undef_cell_sym;

mod undef_cell;

/// RFC 20260710 C2b — the generic immortal `undefined` oddball for
/// pointer-shaped struct slots (Obj / Arr / Closure), a bare
/// `Tag::Undefined` header block in torajs-rc (`undef_cell.rs`).
pub(crate) const UNDEF_CELL_SYM: &str = "___TORAJS_UNDEF_CELL";

/// §7.1.4 — ToNumber(undefined) is NaN. Substitute the constant for
/// an Undefined-typed operand before the lane decision, so the whole
/// downstream (int-vs-float pick, the bool/null coercion arm, the
/// f64 kernels) sees a plain number and needs no undefined story of
/// its own.
///
/// Only the operators whose answer is ToNumber's, and only when BOTH
/// sides are already in the numeric lane. `"x" + u` must stay
/// "xundefined" — its String side owns §13.15.3's concat branch — and
/// an `any` side owns the runtime kernels that carry the walk
/// themselves. The bitwise six are excluded on purpose: ToInt32(NaN)
/// and ToInt32(+0) are both 0, so `undefined | 0` wants null's const
/// 0 and a float round trip would buy nothing. Loose equality is
/// excluded because §7.2.15 step 2 answers a nullish side before
/// ToNumber is ever reached.
///
/// Null is not substituted here: its +0 is representable in the int
/// lane, which is exactly why the coercion family carried the null
/// half for three hundred rotations while `const u = undefined;
/// u * 3` was a compile error.
pub(crate) fn nan_for_undefined_operands(
    ctx: &LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> (Operand, Operand) {
    if !matches!(
        op,
        AstBinOp::Add
            | AstBinOp::Sub
            | AstBinOp::Mul
            | AstBinOp::Div
            | AstBinOp::Mod
            | AstBinOp::Pow
            | AstBinOp::Lt
            | AstBinOp::Gt
            | AstBinOp::Le
            | AstBinOp::Ge
    ) {
        return (a, b);
    }
    // "Already in the numeric lane" is the same question
    // `coerce_bool_null_operands` asks of its own sides: a concrete
    // scalar, or a nullish pointer that its arm turns into one.
    let numeric_lane = |ctx: &LowerCtx<'_>, op: &Operand| {
        matches!(ctx.operand_ty(op), Type::I64 | Type::F64 | Type::Bool)
            || ctx.operand_is_nullish(op)
    };
    if !numeric_lane(ctx, &a) || !numeric_lane(ctx, &b) {
        return (a, b);
    }
    let a = if ctx.binop.left_undef_id.is_some() {
        Operand::ConstF64(f64::NAN)
    } else {
        a
    };
    let b = if ctx.binop.right_undef_id.is_some() {
        Operand::ConstF64(f64::NAN)
    } else {
        b
    };
    (a, b)
}

/// Two operand shapes whose only correct answer is a TypeError, and
/// whose TypeError only the `any`-lane kernels carry: a Symbol
/// (§13.15.3 / §6.1.6 — every arithmetic, ordering and concat
/// operator throws on one) and a BigInt facing a non-BigInt in the
/// numeric families (§6.1.6.1.x's mixed-types refusal).
///
/// The concrete lanes have no arm for either, and never needed one:
/// the checker refuses both typed pairs outright. The `any` spelling
/// reaches them anyway, because `x as any` leaves a heap value's SSA
/// type alone — only primitives and the nullish pointer box there.
/// So `"x" + (Symbol("a") as any)` and `(1n as any) * 2` matched no
/// arm, fell through to the integer lane, and printed the POINTER —
/// the same shape the null coercions printed before their rows were
/// read. Boxing hands the pair to the kernels that have thrown the
/// right TypeError all along: the identical programs through a `const
/// x: any` binding (whose slot really is boxed) already did.
///
/// What stays out is as load-bearing as what goes in. Equality is not
/// in the list — a Symbol compares by identity there, and that arm is
/// correct. Ordering is not in the BigInt half — §7.2.13 IS defined
/// across the numeric types, and `(1n as any) < 2` already answered
/// true. Neither is a String partner, which owns §13.15.3's concat
/// branch and makes `"x" + 1n` the string "x1".
pub(crate) fn box_any_lane_operands(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> (Operand, Operand) {
    let numeric_family = matches!(
        op,
        AstBinOp::Add
            | AstBinOp::Sub
            | AstBinOp::Mul
            | AstBinOp::Div
            | AstBinOp::Mod
            | AstBinOp::Pow
            | AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr
    );
    let ordering = matches!(
        op,
        AstBinOp::Lt | AstBinOp::Gt | AstBinOp::Le | AstBinOp::Ge
    );
    if !numeric_family && !ordering {
        return (a, b);
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    let str_shaped = |t: Type| matches!(t, Type::Str | Type::Substr);
    let mixed_bigint = numeric_family
        && ((a_ty == Type::BigInt) != (b_ty == Type::BigInt))
        && !str_shaped(a_ty)
        && !str_shaped(b_ty);
    let wants_any = |t: Type| t == Type::Symbol || (mixed_bigint && t == Type::BigInt);
    let a = if wants_any(a_ty) {
        ctx.box_to_any(a)
    } else {
        a
    };
    let b = if wants_any(b_ty) {
        ctx.box_to_any(b)
    } else {
        b
    };
    (a, b)
}

impl<'a> LowerCtx<'a> {
    /// RFC 20260710-optional-undefined-repr C1 — materialize the
    /// per-type undefined sentinel address for a sentinel-capable
    /// slot (RFC 20260707 immortal cells; ADRP+ADD, zero calls).
    /// `None` for every other slot type — callers keep whatever
    /// value the undefined literal lowered to (NULL) until that
    /// type's sentinel lands.
    pub(crate) fn str_undef_sentinel_for(&mut self, slot_ty: Type) -> Option<Operand> {
        let sym = undef_cell_sym(slot_ty)?;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::GlobalRef(sym.to_string()),
            slot_ty,
            None,
        );
        Some(Operand::Value(v))
    }

    /// `v == null || v == undefined` for a Str- or Substr-typed value
    /// (§7.2.13 steps 2-3; the ToBoolean falsy pre-probe), inline: NULL,
    /// then the two immortal sentinel addresses (RFC 20260707), each an
    /// ADRP+ADD and a compare. Both sentinels are tested whatever the
    /// slot's static type: a Str slot holds NULL or the Str cell, a
    /// Substr slot the Substr view, and the probe this replaces
    /// (`__torajs_str_is_nullish`) answered both for either.
    pub(crate) fn str_is_nullish(&mut self, v: Operand) -> Operand {
        let ty = self.operand_ty(&v);
        let mut acc = self.cmp(IPred::Eq, v, Operand::ConstPtrNull);
        for sym in [STR_UNDEF_CELL_SYM, SUBSTR_UNDEF_CELL_SYM] {
            let cell = self.f.append_inst(
                self.cur_block,
                InstKind::GlobalRef(sym.to_string()),
                ty,
                None,
            );
            let is_cell = self.cmp(IPred::Eq, v, Operand::Value(cell));
            acc = Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Or, acc, is_cell),
                Type::Bool,
                None,
            ));
        }
        acc
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
    let a_undef = ctx.binop.left_undef_id.is_some();
    let b_undef = ctx.binop.right_undef_id.is_some();
    let a_null = ctx.binop.left_null_id.is_some();
    let b_null = ctx.binop.right_null_id.is_some();
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
                ctx.binop.right_f64_undefable
            } else {
                ctx.binop.left_f64_undefable
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
                        crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64,
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
        // RFC 20260710 C2a/C2b — fn-typed slots (Str-shaped oddball)
        // and refcounted pointer slots (generic Tag::Undefined cell)
        // join the sentinel-capable set.
        if !matches!(val_ty, Type::Str | Type::Substr | Type::FnSig(_))
            && !val_ty.spells_undef_with_generic_cell()
        {
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
