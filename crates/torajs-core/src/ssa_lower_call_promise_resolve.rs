//! `Promise.resolve` / `Promise.reject` static-call lowering — the
//! resolve/reject half of [`crate::ssa_lower_call_promise_static`]
//! (the combinators stay there), pulled into its own sibling when the
//! §27.2.4 static-slot patch consult landed (rotation 448).
//!
//! Every emit is gated on a runtime patch probe over the interned
//! Promise constructor cell's expando dict — the single store the
//! any-lane write (`(Promise as any).resolve = fn`) lands in:
//!
//! 1. **probe** (`promise_ctor_patched`) — emitted FIRST: §13.3.6.1
//!    evaluates the callee reference before Arguments, and the probe
//!    is this site's image of that GetValue. Unpatched programs pay
//!    two loads and a compare.
//! 2. **arguments** — lowered once, in source order, before the
//!    split (the branch itself is not an effect).
//! 3. **fast block** — the pre-existing typed arms verbatim:
//!    zero-arg / undefined-collapse allocator, §27.2.4.7 step 2
//!    promise pass-through, any-lane kernel, heap/primitive
//!    allocator + repr stamp.
//! 4. **patched block** — the call rides the any method dispatcher
//!    against the boxed ctor cell (this = the ctor object, the same
//!    identity `Promise` answers in a value position), then
//!    `promise_patched_result` settles the typed lane's return
//!    contract: a %Promise% cell transfers through, anything else is
//!    a loud catchable TypeError.
//!
//! Both blocks store an OWNED `Type::Promise` into one entry-alloca
//! slot (the ternary dominance pattern); the join loads it.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_resolve_reject(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    method: &str,
    args: &[ExprId],
) -> Operand {
    let name_id: i64 = if method == "resolve" { 0 } else { 1 };
    let cur_block = ctx.cur_block;
    let probe = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_ctor_patched,
            vec![Operand::ConstI64(name_id)],
        ),
        Type::I64,
        None,
    );
    let mut arg_ops: Vec<Operand> = Vec::with_capacity(args.len());
    for &a in args {
        arg_ops.push(ctx.lower_expr(a));
    }
    // §27.2.4.7 — `Promise.resolve(undefined)` settles with exactly
    // what `Promise.resolve()` settles with; the collapse is decided
    // here (compile-time, off the checker's word) so the fast block
    // takes the zero-arg allocator with its void repr stamp. The
    // patched block ignores the collapse — the override receives the
    // real (boxed-undefined) argument.
    let zero_shaped = args.is_empty()
        || (method == "resolve"
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined | crate::check::Type::Void)
            ));
    let cond = ctx.coerce_to_bool(Operand::Value(probe));
    let patched_blk = ctx.f.add_block();
    let fast_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let saved = ctx.cur_block;
    let res_slot = ctx.alloca_in_entry(Type::Promise, Some("__pstat"));

    ctx.cur_block = fast_blk;
    let fast_val = if zero_shaped {
        lower_zero_arg(ctx, method)
    } else {
        lower_one_plus_op(ctx, eid, method, args[0], arg_ops[0].clone())
    };
    let fast_end = ctx.cur_block;
    ctx.f.append_void(
        fast_end,
        InstKind::Store(fast_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(fast_end, Terminator::Br(after_blk));

    ctx.cur_block = patched_blk;
    let patched_val = lower_patched_call(ctx, method, name_id, args, &arg_ops);
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
    let r = ctx.f.append_inst(
        after_blk,
        InstKind::Load(Type::Promise, Operand::Value(res_slot), 0),
        Type::Promise,
        None,
    );
    Operand::Value(r)
}

/// The patched block's body — the any-lane method call against the
/// boxed ctor cell, argv packed from the PRE-LOWERED operands (the
/// per-arg bookkeeping mirrors `pack_any_argv`: borrow-shape args
/// take +1 into the box, fresh owned Any temps ride verbatim and
/// release after the call — the runtime borrows argv).
fn lower_patched_call(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    name_id: i64,
    args: &[ExprId],
    arg_ops: &[Operand],
) -> Operand {
    let cur_block = ctx.cur_block;
    let recv = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_ctor_value,
            vec![Operand::ConstI64(10)],
        ),
        Type::Any,
        None,
    );
    let argc = args.len();
    let argv = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes((argc.max(1) * 8) as u64),
        Type::Ptr,
        Some("__pspatch_argv"),
    );
    let mut drops: Vec<Operand> = Vec::with_capacity(argc);
    for (i, (&aid, op)) in args.iter().zip(arg_ops).enumerate() {
        let ty = ctx.operand_ty(op);
        let slot_val = if ty == Type::Any {
            if ctx.expr_is_fresh_owned(aid) {
                drops.push(op.clone());
            }
            op.clone()
        } else {
            let is_borrow = matches!(
                ctx.ast.get_expr(aid),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Regex { .. }
            );
            if is_borrow && ty.is_refcounted() {
                ctx.emit_rc_inc(op.clone());
            }
            let boxed = ctx.box_to_any_from_expr(aid, op.clone());
            drops.push(boxed.clone());
            boxed
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_void(
            cur_block,
            InstKind::Store(slot_val, Operand::Value(argv), (i * 8) as u64),
        );
    }
    let mid = torajs_rc::any_method_id(method);
    let name_str = ctx.intern_string_literal(method);
    let cur_block = ctx.cur_block;
    let result = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_method_call,
            vec![
                Operand::Value(recv),
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
    // The override can throw (user code) — propagate with the owned
    // result released; the ctor-cell box is a bit-encode over an
    // immortal static cell, no release rides it.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    let cur_block = ctx.cur_block;
    let out = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.promise_patched_result,
            vec![Operand::Value(result), Operand::ConstI64(name_id)],
        ),
        Type::Promise,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(out)
}

/// `Promise.{resolve,reject}()` — synthesize the undefined sentinel
/// and route to the primitive fulfilled/rejected allocator.
fn lower_zero_arg(ctx: &mut LowerCtx<'_>, method: &str) -> Operand {
    let fid = match method {
        "resolve" => ctx.intrinsics.promise_alloc_fulfilled,
        "reject" => ctx.intrinsics.promise_alloc_rejected,
        _ => unreachable!(),
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![Operand::ConstI64(0)]),
        Type::Promise,
        None,
    );
    // RFC 20260720-anylane-promise-methods knife 1 — the zero-arg
    // form settles with undefined; the any-lane bridge answers it
    // as such.
    let out = Operand::Value(v);
    ctx.emit_promise_stamp_repr(&out, crate::ssa_lower_promise_repr_mark::REPR_VOID);
    out
}

/// `Promise.{resolve,reject}(v, ...)` — thenable absorption + heap /
/// primitive dispatch. `arg_op` arrives PRE-LOWERED (the consult
/// scaffold evaluates arguments once, before the split); trailing
/// arguments were already evaluated for effect there too (S322).
fn lower_one_plus_op(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    method: &str,
    arg0: ExprId,
    arg_op: Operand,
) -> Operand {
    let arg_ty = ctx.operand_ty(&arg_op);
    // §27.2.4.7 step 2 — PromiseResolve on a promise whose
    // constructor is %Promise% answers the SAME object
    // (`Promise.resolve(p) === p`; tr has no Promise subclassing, so
    // the pass-through is unconditional). Replaces the pre-spec
    // T-19.f absorption mint (`promise_resolve_thenable` — kernel
    // kept for future any-lane use). Owned-result convention: a
    // borrow-shaped arg shares (+1); an owned temp transfers as-is.
    if matches!(arg_ty, Type::Promise) && method == "resolve" {
        if !ctx.expr_transfers_ownership(arg0) {
            ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
        }
        return arg_op;
    }
    // §27.2.4.7 step 2 through the ANY lane — the runtime kernel
    // probes the box for a %Promise% cell (pass-through, adopting
    // the caller's transferred ref) and otherwise folds the
    // fulfilled_heap + REPR_ANY stamp pair itself, so the
    // pass-through can never be stamped over. Borrow-shaped args
    // share (+1) exactly like the heap lane below.
    if matches!(arg_ty, Type::Any) && method == "resolve" {
        if !ctx.expr_transfers_ownership(arg0) {
            ctx.emit_owned_result_inc(arg_op.clone(), arg_ty);
        }
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![arg_op]),
            Type::Promise,
            None,
        );
        return Operand::Value(v);
    }
    let is_heap = matches!(
        arg_ty,
        Type::Str
            | Type::Substr
            | Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::Any
    );
    let fid = match (method, is_heap) {
        ("resolve", false) => ctx.intrinsics.promise_alloc_fulfilled,
        ("reject", false) => ctx.intrinsics.promise_alloc_rejected,
        ("resolve", true) => ctx.intrinsics.promise_alloc_fulfilled_heap,
        ("reject", true) => ctx.intrinsics.promise_alloc_rejected_heap,
        _ => unreachable!(),
    };
    // `promise_alloc_*_heap` ADOPTS the value (caller transfers one
    // ref, pool.rs contract). A borrow-shaped arg therefore shares:
    // +1 so the source binding keeps its own stake — the old consume
    // path stole it (UAF once the promise dropped first, reuse-window
    // probe printed filler vs bun val42). Owned temps transfer their
    // fresh ref as-is.
    if is_heap && !ctx.expr_transfers_ownership(arg0) {
        ctx.emit_owned_result_inc(arg_op.clone(), arg_ty.clone());
    }
    // RFC 20260720-anylane-promise-methods knife 1 — the repr stamp
    // mirrors the STORED form this site emits (the I64→f64-slot
    // widening below changes it), so capture the predicates the
    // coercion chain is about to consume.
    let is_null = matches!(arg_op, Operand::ConstPtrNull);
    let stored_as_f64 = matches!(arg_ty, Type::F64)
        || (matches!(arg_ty, Type::I64)
            && ctx
                .num_f64_slots
                .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value"));
    mark_arr_value(ctx, &arg_op, &arg_ty);
    let arg_i64 = if matches!(arg_ty, Type::Bool) {
        ctx.coerce_bool_to_i64(arg_op)
    } else if matches!(arg_ty, Type::F64) {
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(arg_op),
            Type::I64,
            None,
        ))
    } else if matches!(arg_ty, Type::I64)
        && ctx
            .num_f64_slots
            .field_is_f64(&crate::num_width::SlotKey::Anon(eid.0), "value")
    {
        let as_f64 = ctx.coerce_to_f64(arg_op);
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::BitCastF64ToI64(as_f64),
            Type::I64,
            None,
        ))
    } else {
        arg_op
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_i64]),
        Type::Promise,
        None,
    );
    let out = Operand::Value(v);
    if let Some(repr) =
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&arg_ty, stored_as_f64, is_null)
    {
        ctx.emit_promise_stamp_repr(&out, repr);
    }
    out
}

/// The repr stamp makes the cell any-consumable, so a typed-Arr
/// value settles as an any-readable cell too: mark its elem kind
/// (RFC 20260704 S1 self-describing posture — the bridge's boxed
/// hand-off is exactly the typed→any crossing the mark exists for).
fn mark_arr_value(ctx: &mut LowerCtx<'_>, arg_op: &Operand, arg_ty: &Type) {
    if matches!(arg_ty, Type::Arr(_)) {
        ctx.emit_arr_mark_kind(arg_op);
    }
}
