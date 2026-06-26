//! `Expr::Ternary { cond, then_branch, else_branch }` lowering
//! pulled out of [`crate::ssa_lower::lower_expr_inner`]'s match arm
//! as chunk-69 of the decomp (chunks 1-68 = ... + `Expr::Closure`
//! M2 env construction).
//!
//! Lowers as `let __tmp; if (cond) __tmp = T else __tmp = E; __tmp`.
//! Result type comes from the branches (verified equal by
//! `check.rs`), with three mixed-arm widen wedges applied **after**
//! both branches lower (so the post-branch `cur_block` reflects any
//! nested ternaries / calls that moved it forward):
//!
//! - **W3 S8** — mixed `i64`/`f64` number pair joins at `f64`.
//!   `check.rs` verified both arms are `number`, but the float face
//!   (`n < 0 ? -n : n`) can leave one arm `f64` while the other
//!   stays `i64`. Whichever is `i64` gets `coerce_to_f64`'d in its
//!   branch's end block before the shared slot store.
//! - **S129-1** — mixed-Any wedge: `check.rs`'s `unify_ternary`
//!   widens to `Any` when either arm is `Any`. The slot must be
//!   `Any`-typed and the non-Any branch NaN-boxed via `box_to_any`
//!   so the post-block Load decodes a proper AnyValue. Same shape
//!   as the S128-5 logical mixed-Any widen and the S128-4 reduce
//!   init box-coerce.
//! - **Other** — fall-through: result type = then arm's type
//!   (caller already verified equality).
//!
//! Both branches store to a single `alloca_in_entry`-allocated
//! result slot; the post-block loads from it. (Same dominance
//! pattern as pending_break — alloca in entry so both branches'
//! current blocks dominate the load.)
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Ternary` match arm bottoms out here).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    cond: ExprId,
    then_branch: ExprId,
    else_branch: ExprId,
) -> Operand {
    let cond_op = ctx.lower_expr(cond);
    let cond_op = ctx.coerce_to_bool(cond_op);
    let then_blk = ctx.f.add_block();
    let else_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let saved = ctx.cur_block;
    ctx.cur_block = then_blk;
    let then_val = ctx.lower_expr(then_branch);
    let then_end = ctx.cur_block;
    ctx.cur_block = else_blk;
    let else_val = ctx.lower_expr(else_branch);
    let else_end = ctx.cur_block;
    let (then_val, else_val, res_ty) = widen_branches(ctx, then_val, else_val, then_end, else_end);
    let res_slot = ctx.alloca_in_entry(res_ty, Some("__tern"));
    ctx.f.append_void(
        then_end,
        InstKind::Store(then_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(then_end, Terminator::Br(after_blk));
    ctx.f.append_void(
        else_end,
        InstKind::Store(else_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(else_end, Terminator::Br(after_blk));
    ctx.f.set_term(
        saved,
        Terminator::CondBr {
            cond: cond_op,
            then_blk,
            else_blk,
        },
    );
    ctx.cur_block = after_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(res_ty, Operand::Value(res_slot), 0),
        res_ty,
        None,
    );
    Operand::Value(r)
}

fn widen_branches(
    ctx: &mut LowerCtx<'_>,
    then_val: Operand,
    else_val: Operand,
    then_end: crate::ssa::BlockId,
    else_end: crate::ssa::BlockId,
) -> (Operand, Operand, Type) {
    let tt = ctx.operand_ty(&then_val);
    let et = ctx.operand_ty(&else_val);
    if tt == Type::F64 && et == Type::I64 {
        ctx.cur_block = else_end;
        let e = ctx.coerce_to_f64(else_val);
        return (then_val, e, Type::F64);
    }
    if tt == Type::I64 && et == Type::F64 {
        ctx.cur_block = then_end;
        let t = ctx.coerce_to_f64(then_val);
        return (t, else_val, Type::F64);
    }
    if tt == Type::Any || et == Type::Any {
        if tt != Type::Any {
            ctx.cur_block = then_end;
            let t = ctx.box_to_any(then_val);
            return (t, else_val, Type::Any);
        }
        if et != Type::Any {
            ctx.cur_block = else_end;
            let e = ctx.box_to_any(else_val);
            return (then_val, e, Type::Any);
        }
        return (then_val, else_val, Type::Any);
    }
    (then_val, else_val, tt)
}
