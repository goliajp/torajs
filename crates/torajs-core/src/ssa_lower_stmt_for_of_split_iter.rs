//! `Stmt::ForOfSplitIter` arm of `LowerCtx::lower_stmt` extracted
//! from [`crate::ssa_lower`] (chunk 150).
//!
//! P-iter — lowers
//!
//! ```text
//! for (let v of <parent>.split(<sep_lit>)) body
//! ```
//!
//! to a SplitIter loop that yields per-iteration STATIC Substr
//! borrows. Baseline layout:
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
//! by the helper / the inline scan below) so rc_inc / rc_dec /
//! substr_drop on `v` no-op — exactly matching the borrow semantics,
//! no per-iter teardown IR.
//!
//! ## S7-r2 — inline Latin-1 fast path (loop versioning)
//!
//! The rpn ablation priced the per-token cross-archive
//! `__torajs_split_iter_next` call at 45% of the whole probe (1.40s
//! → 0.77s with the split deleted; the call target can't inline —
//! S7 立项事实 4-①). When the separator is a compile-time 1-byte
//! Latin-1 literal (the dominant `" "` shape — parser/sfi_pass only
//! mint ForOfSplitIter for literal seps), the loop header instead
//! dispatches each iteration on a HOISTED `parent is Latin-1` bool:
//!
//! - fast arm: inline byte scan (`LDRB` probe per byte via
//!   [`InstKind::LoadU8Dyn`]) + direct Substr write into `sub_slot`,
//!   loop state (`pos` / `done`) in mem2reg-liftable scalar slots.
//!   The iter struct is initialized but its `pos`/`exhausted`
//!   fields are never read on this arm.
//! - slow arm: the pre-existing `split_iter_next` call (UTF-16 /
//!   mixed-encoding parents).
//!
//! The dispatch costs one perfectly-predicted register test per
//! iteration; the two arms converge on the SHARED `body_blk` so the
//! body lowers once and `break`/`continue` semantics are unchanged
//! (`continue` → header re-dispatches on the same hoisted bool).
//! Both arms exit to the same `after:` drop — `split_iter_drop` only
//! reads `parent` + rc_dec's, so fast-arm staleness of the struct's
//! scan fields is unobservable.
//!
//! Fast-arm semantics mirror the kernel's `sep_len == 1` Latin-1
//! branch exactly, including the empty-parent case (one empty chunk
//! is yielded, then exhausted) and the trailing-separator case (an
//! empty tail chunk is yielded when the last byte is the separator).

use crate::ast::{Expr, Stmt};
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
            vec![Operand::Value(iter_slot), parent_op.clone(), sep_op],
        ),
    );

    // Compile-time gate for the inline fast arm: 1-char Latin-1
    // literal separator AND a plain Str parent (a Substr parent's
    // payload is NOT at +16 — its slots 16/24 are parent-ptr /
    // offset, so the inline scan must never fire on it).
    let fast_sep_byte: Option<u8> = match ctx.ast.get_expr(sep) {
        Expr::String(s) if s.chars().count() == 1 => {
            let c = s.chars().next().expect("1-char literal") as u32;
            u8::try_from(c).ok()
        }
        _ => None,
    };
    let fast_sep_byte = fast_sep_byte.filter(|_| ctx.operand_ty(&parent_op) == Type::Str);

    let header = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after = ctx.f.add_block();

    if let Some(target) = fast_sep_byte {
        crate::ssa_lower_stmt_for_of_split_iter_fast::emit_versioned_header(
            ctx, &parent_op, iter_slot, sub_slot, target, header, body_blk, after,
        );
    } else {
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
    }

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
