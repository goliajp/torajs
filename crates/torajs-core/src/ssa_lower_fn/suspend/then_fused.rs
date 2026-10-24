//! `await p.then(f)` in the module body, with `f` a top-level function
//! of one number or boolean parameter returning the await's own lane.
//! §27.2.5.4 queues `f`'s reaction job R on `p` (at once when `p` has
//! settled), and R settles the derived promise D with `f`'s return,
//! which queues the await's continuation C (§27.7.5.3) behind every job
//! pending then. The body awaits in place (`in_place`), so it runs the
//! jobs ahead of R, runs R itself (`f` on `p`'s value), then the jobs
//! ahead of C, and goes on with `f`'s return: the same jobs in the same
//! order, and nothing can see D, so none is made.
//!
//! A `p` whose `then` the program may have replaced takes the call: a
//! cell with an own property bag does at run time (the own-`then`
//! probe of `ssa_lower_promise_chain_own_then`, skipped where its
//! closure proof says no cell can carry one); a call the any-method
//! lane claims (`ssa_lower_any_method_call::claims`: among others, a
//! module that may write `then` on `Promise.prototype` or on
//! `Object.prototype`, `builtin_proto_shadow`) and a module that may
//! redirect SpeciesConstructor are not fused at all. A `p` that is
//! `Promise.resolve` of a value that is not an object is a fresh
//! fulfilled cell: no probe, no wait for R's turn beyond the jobs
//! pending now, no state test. A rejected `p`
//! rejects D with its reason, which the await raises in C; a throw out
//! of `f` does the same.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::{BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, PROMISE_PROPS_OFF};
use crate::ssa_lower_fn::async_main::ModuleCalls;
use crate::ssa_lower_member_promise_value as pv;
use crate::ssa_lower_promise_chain_own_then as own_then;

/// What the fused await needs of `p.then(f)`.
struct Fused {
    recv: ExprId,
    handler: ExprId,
    fid: FuncId,
    param: Type,
    ret: Type,
}

/// The fused lowering of the await `eid` of `obj`, when it applies.
pub(super) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId) -> Option<Operand> {
    let fused = shape(ctx, eid, obj)?;
    Some(lower_fused(ctx, &fused))
}

fn shape(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId) -> Option<Fused> {
    let Expr::Call { callee, args } = ctx.ast.get_expr(obj) else {
        return None;
    };
    let [handler] = args[..] else {
        return None;
    };
    let Expr::Member { obj: recv, name } = ctx.ast.get_expr(*callee) else {
        return None;
    };
    let recv = *recv;
    if name != "then"
        || crate::ssa_lower_any_method_call::claims(ctx, *callee).is_some()
        || !ctx.src_is_builtin_promise(recv)
        || own_then::species_census_written(ctx)
    {
        return None;
    }
    let Expr::Ident(h) = ctx.ast.get_expr(handler) else {
        return None;
    };
    if ctx.locals.contains_key(h) || ctx.globals.contains_key(h) || !plain_one_param_fn(ctx, h) {
        return None;
    }
    let fid = *ctx.fn_table.get(h)?;
    let sig = ctx.fn_sig_ids.get(&fid)?;
    let (params, ret) = ctx.fn_sigs.get(sig.0 as usize)?.clone();
    let [param] = params[..] else {
        return None;
    };
    let scalar = |t: Type| matches!(t, Type::F64 | Type::I64 | Type::I32 | Type::Bool);
    if !scalar(param) || !scalar(ret) || super::inner_lane(ctx, eid, obj) != Some(ret) {
        return None;
    }
    Some(Fused {
        recv,
        handler,
        fid,
        param,
        ret,
    })
}

/// `h` names a top-level function of exactly one plain parameter that
/// is neither a generator nor generic.
fn plain_one_param_fn(ctx: &LowerCtx<'_>, h: &str) -> bool {
    ctx.ast.stmts.iter().any(|s| match s {
        Stmt::FnDecl {
            name,
            type_params,
            params,
            is_generator,
            ..
        } => {
            name == h
                && type_params.is_empty()
                && !is_generator
                && params.len() == 1
                && params[0].default.is_none()
                && !params[0].is_rest
        }
        _ => false,
    })
}

fn lower_fused(ctx: &mut LowerCtx<'_>, fused: &Fused) -> Operand {
    let src = ctx.lower_expr(fused.recv);
    if !ctx.expr_transfers_ownership(fused.recv) {
        ctx.emit_owned_result_inc(src, Type::Promise);
    }
    if fresh_fulfilled(ctx, fused.recv) {
        // R is queued at once, behind the jobs pending now
        let calls = ModuleCalls::of(ctx);
        yield_now(ctx, calls);
        let v = run_reaction(ctx, fused, src, calls);
        ctx.emit_drop_value(src, Type::Promise);
        return v;
    }
    let boxable = own_then::handler_forwarder(ctx, fused.handler).is_some();
    let slot = ctx.alloca_in_entry(fused.ret, Some("__thenv"));
    let join = ctx.f.add_block();
    if boxable
        && !ctx.ast.synth_promise_static_calls.contains(&fused.recv)
        && !own_then::no_cell_overrides(ctx)
    {
        let patched = ctx.f.add_block();
        let fast = ctx.f.add_block();
        let cb = ctx.cur_block;
        let bag = InstKind::Load(Type::Ptr, src, PROMISE_PROPS_OFF);
        let bag = ctx.f.append_inst(cb, bag, Type::Ptr, None);
        let has_bag = InstKind::ICmp(IPred::Ne, Operand::Value(bag), Operand::ConstPtrNull);
        let has_bag = Operand::Value(ctx.f.append_inst(cb, has_bag, Type::Bool, None));
        let term = Terminator::CondBr {
            cond: has_bag,
            then_blk: patched,
            else_blk: fast,
        };
        ctx.f.set_term(cb, term);
        ctx.cur_block = patched;
        let v = lower_called(ctx, fused, src);
        store_and_join(ctx, v, slot, join);
        ctx.cur_block = fast;
    }
    let v = lower_in_place(ctx, fused, src);
    store_and_join(ctx, v, slot, join);
    ctx.cur_block = join;
    let v = InstKind::Load(fused.ret, Operand::Value(slot), 0);
    let v = ctx.f.append_inst(join, v, fused.ret, None);
    ctx.emit_drop_value(src, Type::Promise);
    Operand::Value(v)
}

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, slot: ValueId, join: BlockId) {
    let end = ctx.cur_block;
    let store = InstKind::Store(v, Operand::Value(slot), 0);
    ctx.f.append_void(end, store);
    ctx.f.set_term(end, Terminator::Br(join));
}

/// `recv` is `Promise.resolve(x)` of a value that is not an object, in
/// a module no write can redirect `Promise.resolve` in: a fresh
/// fulfilled cell (§27.2.4.7.1), which no code has seen, so it carries
/// no own `then`.
fn fresh_fulfilled(ctx: &LowerCtx<'_>, recv: ExprId) -> bool {
    use crate::check::Type as T;
    let recv = ctx.peel_as_wrappers(recv);
    let Expr::Call { callee, args } = ctx.ast.get_expr(recv) else {
        return false;
    };
    let [x] = args[..] else {
        return false;
    };
    let Expr::Member { obj, name } = ctx.ast.get_expr(*callee) else {
        return false;
    };
    name == "resolve"
        && matches!(ctx.ast.get_expr(*obj), Expr::Ident(p) if p == "Promise")
        && !ctx.locals.contains_key("Promise")
        && (ctx.ast.synth_promise_static_calls.contains(&recv)
            || ctx.scans.proto_shadow.promise_statics_pinned(ctx.ast))
        && matches!(
            ctx.expr_types.get(&x),
            Some(T::Number | T::String | T::Boolean | T::BigInt)
        )
}

/// The patched `then`: the call through the any lane, then the await
/// of what it answers, read in the await's lane.
fn lower_called(ctx: &mut LowerCtx<'_>, fused: &Fused, src: Operand) -> Operand {
    let h = ctx.lower_expr(fused.handler);
    let token = ctx.push_throw_temp(src, Type::Promise);
    let d = own_then::lower_own_then_invoke(ctx, src, "then", &[fused.handler], &[h], false);
    let v = super::await_cell(ctx, d, None, Some(fused.ret), None, false);
    ctx.pop_throw_temp(token);
    v
}

/// R and C in place: the jobs ahead of R, `f` on the value, the jobs
/// ahead of C. A rejected `src` is raised by C, after the jobs ahead of
/// it. Every throw path out of here releases the owned `src` (the
/// cancel of the wait, `f`'s throw, the raise of a rejection).
fn lower_in_place(ctx: &mut LowerCtx<'_>, fused: &Fused, src: Operand) -> Operand {
    let calls = ModuleCalls::of(ctx);
    let want = pv::awaited_lane_repr(Some(&fused.param));
    let raw = super::in_place::await_read(ctx, src, Some(fused.recv), want, true);
    call_handler(ctx, fused, src, raw, calls)
}

/// R on the fulfilled `src`, `f` on its value, then the jobs queued
/// ahead of C; answers `f`'s return.
fn run_reaction(
    ctx: &mut LowerCtx<'_>,
    fused: &Fused,
    src: Operand,
    calls: ModuleCalls,
) -> Operand {
    let want = pv::awaited_lane_repr(Some(&fused.param));
    let raw = super::read_settled(ctx, src, want);
    call_handler(ctx, fused, src, raw, calls)
}

/// `f` on the value word `raw` read in its parameter's form, then the
/// jobs queued ahead of C; answers `f`'s return.
fn call_handler(
    ctx: &mut LowerCtx<'_>,
    fused: &Fused,
    src: Operand,
    raw: crate::ssa::ValueId,
    calls: ModuleCalls,
) -> Operand {
    let arg = pv::cast_promise_value(ctx, raw, Some(fused.param));
    let b = ctx.cur_block;
    let r = InstKind::Call(fused.fid, vec![Operand::Value(arg)]);
    let r = ctx.f.append_inst(b, r, fused.ret, None);
    rethrow_after_yield(ctx, fused.fid, calls, src);
    yield_now(ctx, calls);
    Operand::Value(r)
}

/// One job, queued now: the jobs ahead of it run.
fn yield_now(ctx: &mut LowerCtx<'_>, calls: ModuleCalls) {
    let b = ctx.cur_block;
    ctx.f
        .append_void(b, InstKind::Call(calls.yield_direct, Vec::new()));
}

/// A throw out of `f` rejects D: the jobs ahead of C run before the
/// await raises it (the throw is kept aside meanwhile).
fn rethrow_after_yield(ctx: &mut LowerCtx<'_>, fid: FuncId, calls: ModuleCalls, src: Operand) {
    if !ctx.may_throw_fns.contains(&ctx.f_name_of(fid)) {
        return;
    }
    let active = ctx.emit_throw_active_load();
    let b = ctx.cur_block;
    let thrown = InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0));
    let thrown = Operand::Value(ctx.f.append_inst(b, thrown, Type::Bool, None));
    let raise = ctx.f.add_block();
    let next = ctx.f.add_block();
    let term = Terminator::CondBr {
        cond: thrown,
        then_blk: raise,
        else_blk: next,
    };
    ctx.f.set_term(b, term);
    ctx.cur_block = raise;
    let call = InstKind::Call(calls.yield_rethrow, Vec::new());
    ctx.f.append_void(raise, call);
    ctx.emit_throw_check_owned(None, src, Type::Promise);
    let end = ctx.cur_block;
    ctx.f.set_term(end, Terminator::Br(next));
    ctx.cur_block = next;
}
