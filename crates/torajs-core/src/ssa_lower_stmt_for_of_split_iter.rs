//! `Stmt::ForOfSplitIter` arm of `LowerCtx::lower_stmt` extracted
//! from [`crate::ssa_lower`] (chunk 150).
//!
//! Pre-extract this arm was 131 LOC inline inside `lower_stmt`.
//! Body verbatim moved here as a free fn; lower_stmt's match arm
//! delegates with one line.
//!
//! P-iter — lowers
//!
//! ```text
//! for (let v of <parent>.split(<sep_lit>)) body
//! ```
//!
//! to a SplitIter loop that yields per-iteration STATIC Substr
//! borrows. Layout:
//!
//! ```text
//! parent_op = lower parent           (Type::Str, ARC-managed)
//! sep_op    = lower sep              (Type::Str, STATIC literal)
//! iter_slot = alloca_bytes 48        (SplitIter struct)
//! sub_slot  = alloca_bytes 32        (Substr borrow)
//! v_slot    = alloca Substr ptr → sub_slot (constant for all iters)
//! call __torajs_split_iter_init(iter_slot, parent_op, sep_op)
//!
//! br header
//! header:
//!   ok = call __torajs_split_iter_next(iter_slot, sub_slot)
//!   cond_br ok, body_blk, after
//! body_blk:
//!   <body — reads of `v` load v_slot → always the same sub_slot
//!     ptr; sub_slot's contents refilled by next() each iter>
//!   br header
//! after:
//!   call __torajs_split_iter_drop(iter_slot)
//! ```
//!
//! `split_iter_init` bumps parent's rc once; `split_iter_drop` dec's
//! it once. Each yielded substr carries the STATIC_LITERAL flag (set
//! by the C helper) so rc_inc / rc_dec / substr_drop on `v` no-op —
//! exactly matching the borrow semantics, no per-iter teardown IR.

use crate::ast::Stmt;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{LocalInfo, LowerCtx};

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    var_name: &str,
    parent: crate::ast::ExprId,
    sep: crate::ast::ExprId,
    body: &Stmt,
) {
    let parent_op = ctx.lower_expr(parent);
    let sep_op = ctx.lower_expr(sep);

    let iter_slot = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::AllocaBytes(48), Type::Ptr, None);
    let sub_slot = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::AllocaBytes(32), Type::Ptr, None);

    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    let v_slot = ctx.alloca(Type::Substr, Some(var_name));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(sub_slot), Operand::Value(v_slot), 0),
    );
    {
        let cur_depth = ctx.scope_stack.len() - 1;
        ctx.locals.insert(
            var_name.to_string(),
            LocalInfo {
                slot: v_slot,
                ty: Type::Substr,
                moved: false,
                borrowed: false,
                scope_depth: cur_depth,
            },
        );
        ctx.scope_stack
            .last_mut()
            .expect("scope frame")
            .push(var_name.to_string());
    }

    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.split_iter_init,
            vec![Operand::Value(iter_slot), parent_op, sep_op],
        ),
    );

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    ctx.f.set_term(ctx.cur_block, Terminator::Br(header));

    ctx.cur_block = header;
    let ok = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.split_iter_next,
            vec![Operand::Value(iter_slot), Operand::Value(sub_slot)],
        ),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(ok),
            then_blk: body_blk,
            else_blk: after,
        },
    );

    ctx.loop_stack.push((header, after));
    ctx.cur_block = body_blk;
    ctx.lower_stmt(body);
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(header));
    }
    ctx.loop_stack.pop();

    ctx.cur_block = after;
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.split_iter_drop,
            vec![Operand::Value(iter_slot)],
        ),
    );

    let _ = ctx.scope_stack.pop().expect("for-of-split scope");
    let _ = ctx.shadow_stack.pop().expect("shadow frame");
    ctx.locals.remove(var_name);
}
