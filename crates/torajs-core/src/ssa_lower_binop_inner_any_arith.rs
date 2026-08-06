//! Any-tagged arith / cmp BinOp dispatch extracted from
//! [`crate::ssa_lower_binop_inner::lower`] (chunk 190 —
//! continuation of file-size debt cleanup, mirrors chunks
//! 185-189 try_lower shape; this chunk gets the source file
//! ≤500 LOC HARD limit).
//!
//! Handles `op ∈ {Add, Sub, Mul, Div, Mod, BitAnd, BitOr, BitXor,
//! Shl, Shr, UShr, Lt, Le, Gt, Ge}` when **at least one operand is
//! `Type::Any`** (P0.6 / P0.7 / P0.8 / RFC 20260716 刀 7). Each
//! operand is packed as `(tag, value-as-i64)` and routed to the
//! matching runtime helper:
//!
//! - **Add** → `any_add` (with ToPrimitive→ToString fallback)
//! - **Sub / Mul / Div / Mod** → `any_arith(op_code, ...)`
//! - **BitAnd / BitOr / BitXor / Shl / Shr / UShr** →
//!   `any_bitwise(op_code, ...)` per ES §13.12 (ToInt32 both
//!   sides, `>>>` uses ToUint32 on LHS)
//! - **Lt / Le / Gt / Ge** → `any_compare(op_code, ...)` (Bool result)
//!
//! Any-typed operand: shim Call `any_unbox_tag` + `any_unbox_value`
//! (Step 7c — was inline +8/+16 direct-offset Load; see
//! ssa_lower.rs head-of-file for the layout-decoupling rationale).
//! Concrete operand: same tag/value packing as box_to_any
//! (Bool zext, F64 bitcast, Ptr null fold to ConstI64(0), etc).
//!
//! Returns `Some(Operand::Value(_))` on hit, `None` when op
//! outside the dispatch set OR neither operand is Any. Caller
//! falls through unchanged on `None`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(
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
            | AstBinOp::Lt
            | AstBinOp::Le
            | AstBinOp::Gt
            | AstBinOp::Ge
    ) {
        return None;
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    if !matches!(a_ty, Type::Any) && !matches!(b_ty, Type::Any) {
        return None;
    }
    // Third slot carries the original Any operand when the pair
    // came from an unbox — the consuming helper only borrows the
    // pair, so a ShortStr-materialized temp needs an
    // `any_unbox_settle` after the call to reclaim its rc.
    let pack =
        |this: &mut LowerCtx, op_v: Operand, op_ty: Type| -> (Operand, Operand, Option<Operand>) {
            if matches!(op_ty, Type::Any) {
                // Any-typed operand: read tag + value via shim.
                // Step 7c: shim Call (was inline +8/+16 direct-offset
                // Load — see ssa_lower.rs head of file for the
                // layout-decoupling rationale).
                let tag = this.f.append_inst(
                    this.cur_block,
                    InstKind::Call(this.intrinsics.any_unbox_tag, vec![op_v.clone()]),
                    Type::I64,
                    None,
                );
                let value = this.f.append_inst(
                    this.cur_block,
                    InstKind::Call(this.intrinsics.any_unbox_value, vec![op_v.clone()]),
                    Type::I64,
                    None,
                );
                return (Operand::Value(tag), Operand::Value(value), Some(op_v));
            }
            // Concrete: same tag/value packing as box_to_any.
            let (tag, value): (i64, Operand) = match op_ty {
                Type::I64 | Type::I32 => (2, op_v),
                Type::F64 => {
                    let bits = this.f.append_inst(
                        this.cur_block,
                        InstKind::BitCastF64ToI64(op_v),
                        Type::I64,
                        None,
                    );
                    (3, Operand::Value(bits))
                }
                Type::Bool => {
                    let zext = this.f.append_inst(
                        this.cur_block,
                        InstKind::ZExtBoolToI64(op_v),
                        Type::I64,
                        None,
                    );
                    (1, Operand::Value(zext))
                }
                Type::Ptr if matches!(op_v, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
                t if t.is_refcounted() => (4, op_v),
                _ => (0, Operand::ConstI64(0)),
            };
            (Operand::ConstI64(tag), value, None)
        };
    let (mut lt, lv, l_src) = pack(ctx, a, a_ty);
    let (mut rt, rv, r_src) = pack(ctx, b, b_ty);
    // RFC 20260716 刀 5 — checker's Undefined flag re-tags a null-
    // shaped pack (Ptr + ConstPtrNull collapses to ANY_NULL=0) as
    // ANY_UNDEF=5 so any_add / any_arith / any_compare see the spec
    // distinction: `s + undefined` concats "undefined", `s + null`
    // concats "null"; ToNumber(undef) = NaN vs ToNumber(null) = 0.
    // Mirrors the P1.5 `Number(undefined) === NaN` shortcut inside
    // ssa_lower_call_coercion for the wrapper-fringe `+`/`</>` sites.
    if ctx.binop.left_undef_id.is_some() && matches!(lt, Operand::ConstI64(0)) {
        lt = Operand::ConstI64(5);
    }
    if ctx.binop.right_undef_id.is_some() && matches!(rt, Operand::ConstI64(0)) {
        rt = Operand::ConstI64(5);
    }
    let settles = [(l_src, lv.clone()), (r_src, rv.clone())];
    let r = match op {
        AstBinOp::Add => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_add, vec![lt, lv, rt, rv]),
            Type::Any,
            None,
        ),
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod | AstBinOp::Pow => {
            // S2.43 — Pow rides the arith kernel (op 4, ES §13.6:
            // ToNumber both sides then Number::exponentiate via the
            // self-ported pow lattice).
            let op_code: i64 = match op {
                AstBinOp::Sub => 0,
                AstBinOp::Mul => 1,
                AstBinOp::Div => 2,
                AstBinOp::Mod => 3,
                AstBinOp::Pow => 4,
                _ => unreachable!(),
            };
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_arith,
                    vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                ),
                Type::Any,
                None,
            )
        }
        AstBinOp::BitAnd
        | AstBinOp::BitOr
        | AstBinOp::BitXor
        | AstBinOp::Shl
        | AstBinOp::Shr
        | AstBinOp::UShr => {
            // RFC 20260716 刀 7 — ES §13.12 bitwise on Any. Op code
            // wire aligned with `arith::BitwiseOp::from_i64` —
            // 0=BitAnd, 1=BitOr, 2=BitXor, 3=Shl, 4=Shr, 5=UShr.
            let op_code: i64 = match op {
                AstBinOp::BitAnd => 0,
                AstBinOp::BitOr => 1,
                AstBinOp::BitXor => 2,
                AstBinOp::Shl => 3,
                AstBinOp::Shr => 4,
                AstBinOp::UShr => 5,
                _ => unreachable!(),
            };
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_bitwise,
                    vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                ),
                Type::Any,
                None,
            )
        }
        AstBinOp::Lt | AstBinOp::Le | AstBinOp::Gt | AstBinOp::Ge => {
            let op_code: i64 = match op {
                AstBinOp::Lt => 0,
                AstBinOp::Le => 1,
                AstBinOp::Gt => 2,
                AstBinOp::Ge => 3,
                _ => unreachable!(),
            };
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_compare,
                    vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                ),
                Type::Bool,
                None,
            )
        }
        _ => unreachable!(),
    };
    for (src, raw) in settles {
        if let Some(orig) = src {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.any_unbox_settle, vec![orig, raw]),
            );
        }
    }
    // The runtime kernels run OrdinaryToPrimitive on object operands
    // (any_add's ToString / the arith-compare ToNumber walks) and can
    // record a catchable TypeError — propagate to the user's
    // try/catch instead of leaking the pending throw past the binop
    // (`"" + objWithShadowedToString` printed the placeholder and
    // the throw surfaced uncaught statements later).
    ctx.emit_throw_check(None);
    Some(Operand::Value(r))
}
