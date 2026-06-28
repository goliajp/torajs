//! Any-tagged arith / cmp BinOp dispatch extracted from
//! [`crate::ssa_lower_binop_inner::lower`] (chunk 190 —
//! continuation of file-size debt cleanup, mirrors chunks
//! 185-189 try_lower shape; this chunk gets the source file
//! ≤500 LOC HARD limit).
//!
//! Handles `op ∈ {Add, Sub, Mul, Div, Mod, Lt, Le, Gt, Ge}`
//! when **at least one operand is `Type::Any`** (P0.6 / P0.7 /
//! P0.8). Each operand is packed as `(tag, value-as-i64)` and
//! routed to the matching runtime helper:
//!
//! - **Add** → `any_add` (with ToPrimitive→ToString fallback)
//! - **Sub / Mul / Div / Mod** → `any_arith(op_code, ...)`
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
    let pack = |this: &mut LowerCtx, op_v: Operand, op_ty: Type| -> (Operand, Operand) {
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
                InstKind::Call(this.intrinsics.any_unbox_value, vec![op_v]),
                Type::I64,
                None,
            );
            return (Operand::Value(tag), Operand::Value(value));
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
        (Operand::ConstI64(tag), value)
    };
    let (lt, lv) = pack(ctx, a, a_ty);
    let (rt, rv) = pack(ctx, b, b_ty);
    let r = match op {
        AstBinOp::Add => ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_add, vec![lt, lv, rt, rv]),
            Type::Any,
            None,
        ),
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod => {
            let op_code: i64 = match op {
                AstBinOp::Sub => 0,
                AstBinOp::Mul => 1,
                AstBinOp::Div => 2,
                AstBinOp::Mod => 3,
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
    Some(Operand::Value(r))
}
