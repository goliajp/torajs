//! The module body's await (`ast::desugar_async_ssa_gate` set
//! `AsyncSsa::main`): the body is the root of every job's stack, so it
//! runs the jobs ahead of its continuation in place and goes on
//! (torajs-promise `module_await`), with no frame and no suspension
//! point.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_fn::async_main::ModuleCalls;

/// The lowering is the module body's.
pub(super) fn in_module_body(ctx: &LowerCtx<'_>) -> bool {
    ctx.is_main_fn && ctx.suspend_body
}

/// `torajs-promise` `AWAIT_FUSED`.
const AWAIT_FUSED: i64 = 1;

/// The await of the owned cell `src` in the module body, and the read
/// of its settled value word in the form `want_repr` names. The body
/// keeps its reference on `src`; `obj` is the operand `src` is the
/// value of, when there is one. The result of a call to an async
/// function that suspends over a frame and returns, in the form read, a
/// value that cannot be adopted is settled only by that frame's return
/// or throw, the last call of its job; when the operand is that call
/// itself ([`frame_result`]), only this await sees it: the body runs
/// jobs until its state byte is set, with no waiter parked
/// (`__torajs_microtask_await_byte`). Any other promise gets one
/// (`__torajs_module_await_promise`). Either answers 1 when the cell
/// is fulfilled in that form, which is read in place; anything else
/// (another form, a rejection, a queue run dry) goes through
/// `__torajs_module_await_slow`, whose throws (the reason, the cancel
/// throw) take the path out that releases `src`. `fused`: `src` is the
/// receiver of a fused `await p.then(f)` (`then_fused`).
pub(super) fn await_read(
    ctx: &mut LowerCtx<'_>,
    src: Operand,
    obj: Option<ExprId>,
    want_repr: i64,
    fused: bool,
) -> ValueId {
    let calls = ModuleCalls::of(ctx);
    let cb = ctx.cur_block;
    let fresh = obj.and_then(|obj| frame_result(ctx, obj));
    let (call, slow_fn) = if fresh == Some(want_repr) {
        let base = InstKind::PtrToInt(src);
        let base = ctx.f.append_inst(cb, base, Type::I64, None);
        let off = Operand::ConstI64(super::read::PROMISE_STATE_WORD_OFF as i64);
        let at = InstKind::BinOp(BinOp::Add, Operand::Value(base), off);
        let at = ctx.f.append_inst(cb, at, Type::I64, None);
        let at = InstKind::IntToPtr(Operand::Value(at));
        let at = Operand::Value(ctx.f.append_inst(cb, at, Type::Ptr, None));
        // the state byte: 1 is a fulfillment in the form read
        (
            InstKind::Call(calls.await_byte, vec![at]),
            calls.await_slow_frame,
        )
    } else {
        let (mask, want) = super::read::state_word_wanted(want_repr);
        let word = [
            Operand::ConstI32(mask as i32),
            Operand::ConstI32(want as i32),
        ];
        let args = vec![src, word[0], word[1]];
        (InstKind::Call(calls.await_promise, args), calls.await_slow)
    };
    let code = ctx.f.append_inst(cb, call, Type::I64, None);
    let hit = InstKind::ICmp(IPred::Eq, Operand::Value(code), Operand::ConstI64(1));
    let hit = ctx.f.append_inst(cb, hit, Type::Bool, None);
    let slot = ctx.alloca_in_entry(Type::I64, Some("__awaitv"));
    let in_place = ctx.f.add_block();
    let slow = ctx.f.add_block();
    let join = ctx.f.add_block();
    branch(ctx, cb, hit, in_place, slow);
    let v = InstKind::Load(Type::I64, src, super::read::PROMISE_VALUE_OFF);
    let v = ctx.f.append_inst(in_place, v, Type::I64, None);
    store_and_br(ctx, in_place, v, slot, join);
    ctx.cur_block = slow;
    let args = vec![
        src,
        Operand::Value(code),
        Operand::ConstI64(if fused { AWAIT_FUSED } else { 0 }),
        Operand::ConstI64(want_repr),
    ];
    let raw = InstKind::Call(slow_fn, args);
    let raw = ctx.f.append_inst(slow, raw, Type::I64, None);
    ctx.emit_throw_check_owned(None, src, Type::Promise);
    let end = ctx.cur_block;
    store_and_br(ctx, end, raw, slot, join);
    ctx.cur_block = join;
    let v = InstKind::Load(Type::I64, Operand::Value(slot), 0);
    ctx.f.append_inst(join, v, Type::I64, None)
}

fn store_and_br(ctx: &mut LowerCtx<'_>, b: BlockId, v: ValueId, slot: ValueId, to: BlockId) {
    let store = InstKind::Store(Operand::Value(v), Operand::Value(slot), 0);
    ctx.f.append_void(b, store);
    ctx.f.set_term(b, Terminator::Br(to));
}

fn branch(ctx: &mut LowerCtx<'_>, b: BlockId, cond: ValueId, then_blk: BlockId, else_blk: BlockId) {
    let cond = Operand::Value(cond);
    let term = Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
    ctx.f.set_term(b, term);
}

/// `super::suspend_direct` in the module body: one job, queued at once.
pub(super) fn yield_direct(ctx: &mut LowerCtx<'_>) {
    let calls = ModuleCalls::of(ctx);
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Call(calls.yield_direct, Vec::new()));
}

/// When the operand `obj` is itself a direct call of an async function
/// that suspends over a frame (past `as` and a comma's left operand),
/// handing its result over, in a form that cannot be adopted: that
/// form. Only then has no other code seen the promise: an assignment
/// (`await (p = f())`) answers the same cell its target keeps, and a
/// read of a binding or a field answers one that is held elsewhere.
fn frame_result(ctx: &LowerCtx<'_>, obj: ExprId) -> Option<i64> {
    use crate::ssa_lower_promise_repr_mark::{REPR_ANY, REPR_HEAP};
    if !ctx.expr_transfers_ownership(obj) {
        return None;
    }
    let mut e = obj;
    while let Expr::As { expr, .. } | Expr::Sequence { right: expr, .. } = ctx.ast.get_expr(e) {
        e = *expr;
    }
    let Expr::Call { callee, .. } = ctx.ast.get_expr(e) else {
        return None;
    };
    let Expr::Ident(name) = ctx.ast.get_expr(*callee) else {
        return None;
    };
    if ctx.locals.contains_key(name) || ctx.globals.contains_key(name) {
        return None;
    }
    let fid = *ctx.fn_table.get(name)?;
    let repr = *ctx.scans.frame_result_reprs.get(&fid)?;
    (!matches!(repr, REPR_HEAP | REPR_ANY)).then_some(repr)
}
