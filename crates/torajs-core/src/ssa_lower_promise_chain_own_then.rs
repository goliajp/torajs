//! Rotation 652 — the own-`then` split of a typed chain call.
//!
//! `p.then(f)` is `Invoke(p, "then", «f»)`: when the cell carries an
//! own `then` (a user store `p.then = g` through the any lane lands in
//! the cell's expando bag), the call is the override's, and
//! §27.2.5.4 — which the typed `then` kernels implement — is only the
//! builtin's body. The any lane already honours this (its own-bag
//! gate); the typed lane attached straight to the kernels, so
//! `Promise.resolve(p).then(cb)` over a patched `p` skipped the
//! override that the same program spelled `p.then(cb)` ran (test262
//! `Promise/*/invoke-then*`, `resolve-thenable-*`).
//!
//! Same shape as the §27.2.4 static-slot patch consult
//! (`ssa_lower_call_promise_resolve`): an inline NULL check of the
//! cell's expando-bag slot first (rotation 653; since rotation 660 a
//! bag alone sends the call to the any lane, which also answers
//! §27.2.5.4's SpeciesConstructor), the handlers lowered once ahead of the
//! split, a fast block with the pre-existing typed emit, a patched
//! block that rides the any method dispatcher against the boxed cell
//! and settles the typed return contract through
//! `promise_patched_result`, one entry-alloca slot the join loads.
//! `.catch` / `.finally` Invoke `then` per spec too (rotation 654):
//! their patched block dispatches the call's own name, and the
//! runtime's Promise arm turns it into the Invoke.
//!
//! The typed return contract has one exception, and it is the whole
//! of what `discarded` carries: §27.2.5.1 lets the override answer
//! anything, and `promise_patched_result` makes that loud because a
//! `Type::Promise` slot cannot hold it. In statement position there
//! is no slot — `Promise.resolve(p).then(f)` as a whole statement
//! reads nothing back — so the answer is released and the join loads
//! a null Promise. Every other position keeps the loud refusal: it is
//! better than a silently wrong `typeof`, which is what answering a
//! stand-in promise would give (test262
//! `Promise/resolve/resolve-prms-cstm-then`).
//!
//! The async machinery's driver (`__w.then(...)` over the
//! `__torajs_await_resolve` result, rotation 651-05) takes no probe:
//! it is §27.7.5.3 Await = PerformPromiseThen, which never reads
//! `then` — bun agrees (`await p` over a patched `p` runs no
//! override). The desugar marks that receiver Ident in
//! `synth_promise_static_calls` (the driver text goes through the
//! lexer, so the source-dunder judge would call `__w` the program's).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, PROMISE_PROPS_OFF};

pub(crate) fn lower_chain_with_own_then_probe(
    ctx: &mut LowerCtx<'_>,
    src_op: Operand,
    src_id: ExprId,
    m_name: &str,
    args: &[ExprId],
    arg_ops: &[Operand],
    discarded: bool,
) -> ValueId {
    // A handler the patched block could not box — a raw FnSig with no
    // forwarder minted for it — keeps the site on the typed lane it
    // had before the split (registered: the collector mints one for
    // every top-FnDecl handler, so this is the residual shape).
    let boxable = args.iter().zip(arg_ops).all(|(&aid, op)| {
        !matches!(ctx.operand_ty(op), Type::FnSig(_)) || handler_forwarder(ctx, aid).is_some()
    });
    if !boxable || ctx.ast.synth_promise_static_calls.contains(&src_id) {
        return ctx.lower_chain_fast(m_name, src_op, args, arg_ops);
    }
    let patched_blk = ctx.f.add_block();
    let fast_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let res_slot = ctx.alloca_in_entry(Type::Promise, Some("__pthen"));
    // Two things can make the builtin kernels the wrong body: an own
    // `then` (§27.2.5.1 Invoke) and a SpeciesConstructor other than
    // %Promise% (§27.2.5.4 steps 3-4). Per cell, both need the
    // expando bag, which is NULL until an any-lane store lands — so
    // the unpatched chain pays one load and a branch, and a cell with
    // a bag rides the any dispatcher, whose Promise arm answers both.
    // Program-wide, a write that can reach `Promise.prototype
    // .constructor` or a `@@species` accessor redirects every cell
    // with no bag at all; the census says whether this module has one.
    let entry = ctx.cur_block;
    let saved = entry;
    let cond = if species_census_written(ctx) {
        Operand::ConstBool(true)
    } else {
        let bag = ctx.f.append_inst(
            entry,
            InstKind::Load(Type::Ptr, src_op, PROMISE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let has_bag = ctx.f.append_inst(
            entry,
            InstKind::ICmp(IPred::Ne, Operand::Value(bag), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        Operand::Value(has_bag)
    };

    ctx.cur_block = fast_blk;
    let fast_val = ctx.lower_chain_fast(m_name, src_op, args, arg_ops);
    let fast_end = ctx.cur_block;
    ctx.f.append_void(
        fast_end,
        InstKind::Store(Operand::Value(fast_val), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(fast_end, Terminator::Br(after_blk));

    ctx.cur_block = patched_blk;
    let patched_val = lower_own_then_invoke(ctx, src_op, m_name, args, arg_ops, discarded);
    let patched_end = ctx.cur_block;
    ctx.f.append_void(
        patched_end,
        InstKind::Store(patched_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(patched_end, Terminator::Br(after_blk));

    ctx.f.set_term(
        saved,
        Terminator::CondBr {
            cond,
            then_blk: patched_blk,
            else_blk: fast_blk,
        },
    );
    ctx.cur_block = after_blk;
    ctx.f.append_inst(
        after_blk,
        InstKind::Load(Type::Promise, Operand::Value(res_slot), 0),
        Type::Promise,
        None,
    )
}

/// Might this module redirect SpeciesConstructor for a bag-less cell?
/// `Promise.prototype.constructor` reached by a write, any `@@species`
/// handler written (`defineProperty(Promise, Symbol.species, …)` lands
/// in the constructor's own dict, where no cell can see it), or a
/// class inheriting from Promise — its instances flow into slots the
/// checker types `Promise` (`P.resolve(x)`), and their species is the
/// subclass.
pub(crate) fn species_census_written(ctx: &LowerCtx<'_>) -> bool {
    let set = &ctx.scans.proto_shadow;
    set.shadows("Promise", "constructor")
        || set.hook_written("species")
        || ctx
            .ast
            .builtin_class_parents
            .iter()
            .any(|(_, p)| p == "Promise")
}

/// `Some("__fwdrecv_<f>" | "__forward_<f>")` when `aid` names a
/// top-level fn whose
/// forwarder the collector minted (`ast_collect_fn_closure_call::
/// mark_promise_chain_handler_targets`).
pub(crate) fn handler_forwarder(ctx: &LowerCtx<'_>, aid: ExprId) -> Option<String> {
    let Expr::Ident(f) = ctx.ast.get_expr(ctx.peel_as_wrappers(aid)) else {
        return None;
    };
    crate::ast::shell_names(f)
        .into_iter()
        .find(|n| ctx.fn_table.contains_key(n.as_str()))
}

/// The override's call: the receiver cell boxed as a borrow (the
/// chain caller owns or borrows `src_op` and settles it after the
/// join), each handler boxed the way the static-slot patch boxes its
/// arguments, the any method dispatcher under the call's own method
/// name (`catch` / `finally` reach the runtime's Invoke-of-`then`
/// lane there, rotation 654), then the typed return contract.
pub(crate) fn lower_own_then_invoke(
    ctx: &mut LowerCtx<'_>,
    src_op: Operand,
    m_name: &str,
    args: &[ExprId],
    arg_ops: &[Operand],
    discarded: bool,
) -> Operand {
    ctx.emit_rc_inc(src_op);
    let recv = ctx.box_to_any(src_op);
    let argc = args.len();
    let argv = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes((argc.max(1) * 8) as u64),
        Type::Ptr,
        Some("__pthen_argv"),
    );
    let mut drops: Vec<Operand> = Vec::with_capacity(argc + 1);
    for (i, (&aid, op)) in args.iter().zip(arg_ops).enumerate() {
        let ty = ctx.operand_ty(op);
        let slot_val = if let Type::FnSig(_) = ty {
            // The typed lane holds the bare address; the override
            // needs a cell. The collector minted `__forward_<f>` for
            // this handler (checked before the split), and a
            // forwarder has no captures.
            let fwd = handler_forwarder(ctx, aid).expect("checked by the boxable gate");
            let cell = crate::ssa_lower_closure::lower(ctx, fwd, Vec::new());
            let boxed = ctx.box_to_any(cell);
            drops.push(boxed);
            boxed
        } else if ty == Type::Any {
            if ctx.expr_is_fresh_owned(aid) {
                drops.push(*op);
            }
            *op
        } else {
            let is_borrow = !ctx.expr_is_fresh_owned(aid);
            let boxed = ctx.box_owned_from_expr(aid, *op, is_borrow);
            drops.push(boxed);
            boxed
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(slot_val, Operand::Value(argv), (i * 8) as u64),
        );
    }
    let mid = torajs_rc::any_method_id(m_name);
    let name_str = ctx.intern_string_literal(m_name);
    let cur_block = ctx.cur_block;
    let result = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                recv,
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                Operand::ConstI64(0),
                Operand::ConstPtrNull,
                Operand::Value(argv),
                Operand::ConstI64(argc as i64),
            ],
        ),
        Type::Any,
        None,
    );
    for d in drops {
        ctx.emit_drop_value(d, Type::Any);
    }
    ctx.emit_drop_value(recv, Type::Any);
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    // No slot, no contract: an expression statement reads nothing back,
    // so the override's answer is released and the join loads a null
    // Promise (its drop is a no-op). `promise_patched_result` refusing
    // a non-promise is about what a `Type::Promise` SLOT can hold, and
    // here there is none.
    if discarded {
        ctx.emit_drop_value(Operand::Value(result), Type::Any);
        return Operand::ConstPtrNull;
    }
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_patched_result,
            vec![Operand::Value(result), Operand::ConstI64(2)],
        ),
        Type::Promise,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(out)
}
