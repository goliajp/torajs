//! `Expr::OptCall { callee, args }` lowering — `callee?.(args…)`
//! optional call (ES2020 §13.3.9, chunk 705). The OptChain sibling
//! owns `?.field`, OptIndex owns `?.[index]`; this file owns the
//! call shape.
//!
//! Same contract as the siblings: the callee evaluates exactly once;
//! a nullish callee short-circuits to an ANY_UNDEF box WITHOUT
//! evaluating the arguments (they lower inside the hit block); a
//! non-nullish non-callable callee is the runtime `__torajs_any_call`
//! catchable TypeError. Four callee routes, keyed off the checker
//! type (mirrors `check_type_of_misc::check_opt_call`):
//!
//! - **plain** (statically non-nullish) — `?.()` ≡ `()`: delegate to
//!   the regular Call dispatcher verbatim (`g?.(21)` on a closure
//!   binding rides the typed fast path, result keeps its concrete
//!   type).
//! - **Null / Undefined** — statically nullish: the callee lowers
//!   for side effects, the args never lower, the result is a
//!   constant ANY_UNDEF box.
//! - **Member callee on an `any` receiver** (chunk 709) — GetV-
//!   existence probe + `__torajs_any_method_call_opt`: builtin
//!   methods dispatch (`s.toUpperCase?.()`), no-such answers
//!   undefined, resolved non-callables throw. See
//!   [`lower_member_opt`].
//! - **Any** (non-Member callee) — NaN-box tag guard (shared
//!   `emit_nullish_cond`); the hit path packs the argv
//!   (`pack_any_argv` three-shape ledger) and dispatches
//!   `__torajs_any_call`.
//! - **Nullable** — raw ptr null guard; the hit path boxes the
//!   unwrapped value (borrow inc / owned transfer per the pack
//!   ledger) and dispatches `__torajs_any_call` (a non-closure
//!   inner, e.g. `Arr<Str> | null`, is its catchable TypeError).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_any_method_call::pack_any_argv;
use crate::ssa_lower_optindex::emit_nullish_cond;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    // Chunk 709 — a Member callee on an `any` receiver dispatches
    // through the opt method-call runtime so builtin methods work
    // (`s.toUpperCase?.()` — a member READ answers undefined for
    // builtin names, chunk-521 boundary) with correct `this`-style
    // receiver dispatch and the ES no-such → undefined short-circuit.
    if let crate::ast::Expr::Member { obj, name } = ctx.ast.get_expr(callee)
        && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any))
    {
        let (obj, name) = (*obj, name.clone());
        return lower_member_opt(ctx, obj, &name, args);
    }
    match ctx.expr_types.get(&callee) {
        Some(crate::check::Type::Null) | Some(crate::check::Type::Undefined) => {
            let v = ctx.lower_expr(callee);
            ctx.release_owned_temp(callee, &v);
            emit_undef_box(ctx)
        }
        Some(crate::check::Type::Any) | Some(crate::check::Type::Nullable(_)) => {
            lower_guarded(ctx, callee, args)
        }
        _ => crate::ssa_lower_call::lower(ctx, eid, callee, args),
    }
}

/// `o.m?.(args…)` with `o: any` — GetV-existence probe, then the
/// opt method-call dispatch. ES §13.3.9 order: the receiver
/// evaluates once, the probe decides the branch (a nullish receiver
/// throws inside the probe; a nullish/absent property short-circuits
/// to undefined WITHOUT evaluating the args), and the hit path packs
/// the argv and dispatches `__torajs_any_method_call_opt` (a builtin
/// mid rides the id switch; a residual no-such answers undefined; a
/// resolved non-callable is the catchable TypeError).
fn lower_member_opt(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str, args: &[ExprId]) -> Operand {
    let mid = torajs_rc::any_method_id(name);
    let name_str = ctx.intern_string_literal(name);
    let recv = ctx.lower_expr(obj);
    // Rotation 550 — an owned receiver is live across the probe's
    // throw edge, the hit path's argument lowers and the dispatch;
    // park it until its release at the join.
    let recv_tok = ctx.park_owned_temp(obj, &recv);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optcall_m"));
    let exists = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_probe,
            vec![
                recv.clone(),
                Operand::ConstI64(mid),
                Operand::Value(name_str),
            ],
        ),
        Type::I64,
        None,
    );
    // The probe records a catchable TypeError for a nullish
    // receiver (`o.m` evaluation throws per ES) — propagate before
    // branching.
    ctx.emit_throw_check(None);
    let cond = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            crate::ssa::IPred::Eq,
            Operand::Value(exists),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );
    ctx.cur_block = null_blk;
    let undef_box = emit_undef_box(ctx);
    ctx.f.append_void(
        null_blk,
        InstKind::Store(undef_box, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(null_blk, Terminator::Br(after));
    // Hit path — the args lower HERE (short-circuit never evaluates
    // them). recv_slot mirrors the plain method-call arm: an Ident
    // receiver rides its variable slot so growth-relocating methods
    // (push) write the fresh block pointer back.
    ctx.cur_block = hit_blk;
    let recv_slot = if let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(obj) {
        if let Some(info) = ctx.locals.get(n) {
            Operand::Value(info.slot)
        } else if ctx.globals.contains_key(n) {
            let gname = n.clone();
            let gref =
                ctx.f
                    .append_inst(ctx.cur_block, InstKind::GlobalRef(gname), Type::Ptr, None);
            Operand::Value(gref)
        } else {
            Operand::ConstPtrNull
        }
    } else {
        Operand::ConstPtrNull
    };
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call_opt,
            vec![
                recv.clone(),
                Operand::ConstI64(mid),
                Operand::Value(name_str),
                recv_slot,
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    packed.release(ctx);
    // result is an OWNED Any already in hand — the throw path must
    // release it (mirrors the plain method-call arm).
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(result), Operand::Value(res_slot), 0),
    );
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    ctx.unpark_owned_temp(recv_tok);
    ctx.release_owned_temp(obj, &recv);
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

fn lower_guarded(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    let transfers = ctx.expr_transfers_ownership(callee);
    let v = ctx.lower_expr(callee);
    let v_ty = ctx.operand_ty(&v);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optcall"));
    let nullish = emit_nullish_cond(ctx, &v, &v_ty);
    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: nullish,
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );
    // Miss path → ANY_UNDEF box. A ptr-typed owned callee temp
    // (Nullable fn-call result) was never consumed here — release
    // its ref (dead when v is statically non-null).
    ctx.cur_block = null_blk;
    let undef_box = emit_undef_box(ctx);
    ctx.f.append_void(
        null_blk,
        InstKind::Store(undef_box, Operand::Value(res_slot), 0),
    );
    if !matches!(v_ty, Type::Any) && transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v.clone(), v_ty.clone());
    }
    ctx.f.set_term(null_blk, Terminator::Br(after));
    // Hit path — the arguments lower HERE (short-circuit never
    // evaluates them).
    ctx.cur_block = hit_blk;
    let (callee_av, we_boxed) = if matches!(v_ty, Type::Any) {
        (v.clone(), false)
    } else {
        // pack ledger: box_to_any is TRANSFER — a borrowed value
        // rc-incs first, an owned temp hands its ref to the box.
        if !transfers && v_ty.is_refcounted() {
            ctx.emit_rc_inc(v.clone());
        }
        (ctx.box_to_any(v.clone()), true)
    };
    // Rotation 550 — the callee we hold (the box we minted, or an
    // owned already-Any temp released at the join) is live across
    // the arguments' lowers; park it for their throw paths.
    let callee_tok = if we_boxed {
        Some(ctx.push_throw_temp(callee_av.clone(), Type::Any))
    } else {
        ctx.park_owned_temp(callee, &v)
    };
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_call,
            vec![
                callee_av.clone(),
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );
    packed.release(ctx);
    ctx.unpark_owned_temp(callee_tok);
    if we_boxed {
        ctx.emit_drop_value(callee_av, Type::Any);
    }
    // result is an OWNED Any already in hand — the throw path must
    // release it (mirrors the plain method-call arm).
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(result), Operand::Value(res_slot), 0),
    );
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    // An Any-typed owned callee temp (member read / chained call)
    // was borrowed by the dispatch on the hit path and untouched on
    // the miss path — one release covers both.
    if matches!(v_ty, Type::Any) {
        ctx.release_owned_temp(callee, &v);
    }
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

/// Constant `undefined` result box (spec: the short-circuit value is
/// undefined, not null).
fn emit_undef_box(ctx: &mut LowerCtx<'_>) -> Operand {
    let b = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(b)
}
