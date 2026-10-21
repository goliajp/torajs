//! `Promise.resolve` / `Promise.reject` static-call lowering — the
//! resolve/reject half of [`crate::ssa_lower_call_promise_static`]
//! (the combinators stay there), pulled into its own sibling when the
//! §27.2.4 static-slot patch consult landed (rotation 448).
//!
//! Every emit is gated on a runtime patch probe over the interned
//! Promise constructor cell's expando dict — the single store the
//! any-lane write (`(Promise as any).resolve = fn`) lands in — unless
//! the program provably writes no such slot
//! (`ShadowSet::promise_statics_pinned`):
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

mod patched;

use patched::lower_patched_call;

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower_resolve_reject(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    method: &str,
    args: &[ExprId],
) -> Operand {
    // The async desugar's synthesized settle spellings are the
    // machinery's own §27.7 capability operations, never the user's
    // patchable slot — no probe, straight to the typed lane. Neither
    // is there one to probe in a program that cannot write the slot.
    if ctx.ast.synth_promise_static_calls.contains(&eid)
        || ctx.scans.proto_shadow.promise_statics_pinned(ctx.ast)
    {
        return lower_unprobed(ctx, eid, method, args);
    }
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
    let zero_shaped = zero_shaped(ctx, method, args);
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
        lower_one_plus_op(ctx, eid, method, args[0], arg_ops[0])
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

/// The typed lane alone, for a call no program write can redirect.
fn lower_unprobed(ctx: &mut LowerCtx<'_>, eid: ExprId, method: &str, args: &[ExprId]) -> Operand {
    let mut arg_ops: Vec<Operand> = Vec::with_capacity(args.len());
    for &a in args {
        arg_ops.push(ctx.lower_expr(a));
    }
    if zero_shaped(ctx, method, args) {
        lower_zero_arg(ctx, method)
    } else {
        lower_one_plus_op(ctx, eid, method, args[0], arg_ops[0])
    }
}

/// §27.2.4.7 — `Promise.resolve(undefined)` settles with exactly
/// what `Promise.resolve()` settles with; the collapse is decided at
/// compile time off the checker's word, so the fast lane takes the
/// zero-arg allocator with its void repr stamp. The patched block
/// ignores the collapse — the override receives the real
/// (boxed-undefined) argument.
fn zero_shaped(ctx: &LowerCtx<'_>, method: &str, args: &[ExprId]) -> bool {
    args.is_empty()
        || (method == "resolve"
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined | crate::check::Type::Void)
            ))
}

/// `Promise.{resolve,reject}()` — synthesize the undefined sentinel
/// and route to the primitive fulfilled/rejected allocator.
fn lower_zero_arg(ctx: &mut LowerCtx<'_>, method: &str) -> Operand {
    // RFC 20260720-anylane-promise-methods knife 1 — the zero-arg
    // form settles with undefined; the any-lane bridge answers it
    // as such.
    use crate::ssa_lower_promise_repr_mark::REPR_VOID;
    let fid = match method {
        "resolve" => {
            return ctx.emit_promise_alloc_fulfilled_repr(Operand::ConstI64(0), false, REPR_VOID);
        }
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
    let out = Operand::Value(v);
    ctx.emit_promise_stamp_repr(&out, REPR_VOID);
    out
}

/// Does a typed object's static shape carry a `then` member — a field
/// in its struct layout (an object literal's method is a closure
/// field), or a class method by name? Only a `Type::Obj` can; every
/// other typed form has no `then` to find.
fn typed_shape_has_then(ctx: &LowerCtx<'_>, arg0: ExprId, arg_ty: &Type) -> bool {
    let Type::Obj(sid) = arg_ty else {
        return false;
    };
    ctx.struct_layouts[sid.0 as usize]
        .iter()
        .any(|(k, _)| *k == "then")
        || crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, arg0)
            .is_some_and(|c| ctx.fn_table.contains_key(&format!("__cm_{c}__then")))
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
    // §27.2.4.7.1 steps 1-2 — PromiseResolve on a native promise
    // answers the SAME object only when `x.constructor` is %Promise%.
    // The typed arm used to pass through unconditionally ("tr has no
    // Promise subclassing"), which stopped being true: a subclass
    // instance, or a promise whose `constructor` expando was
    // reassigned (test262 `arg-uniq-ctor` spells it on a typed
    // binding), must get a fresh promise that adopts it. The any-lane
    // kernel makes that decision (rotation 650), so a typed promise
    // rides it too — a cell boxes as its pointer, no allocation, and
    // the pass-through is still the kernel's answer for the plain
    // case. Owned-result convention: a borrow-shaped arg shares (+1);
    // an owned temp transfers as-is. The kernel adopts the caller's
    // transferred ref and otherwise folds the fulfilled_heap +
    // REPR_ANY stamp pair itself, so the pass-through can never be
    // stamped over.
    if matches!(arg_ty, Type::Promise | Type::Any) && method == "resolve" {
        if !ctx.expr_transfers_ownership(arg0) {
            ctx.emit_owned_result_inc(arg_op, arg_ty);
        }
        let boxed = if arg_ty == Type::Any {
            arg_op
        } else {
            ctx.box_to_any_from_expr(arg0, arg_op)
        };
        // an async function's own result is a fresh promise that adopts
        // a returned one (§27.7.5.1), not the returned one itself
        let kernel = if ctx.ast.synth_promise_static_calls.contains(&eid) {
            ctx.intrinsics.promise_resolve_fresh
        } else {
            ctx.intrinsics.promise_resolve_any
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(kernel, vec![boxed]),
            Type::Promise,
            None,
        );
        return Operand::Value(v);
    }
    // §27.2.4.7 through §27.2.1.3.2 — a typed object whose shape carries
    // a `then` member is a thenable: box it and take the any-lane
    // kernel (the job road). A shape without one is proven
    // non-thenable here, at compile time, and keeps the direct mint.
    if method == "resolve" && typed_shape_has_then(ctx, arg0, &arg_ty) {
        if !ctx.expr_transfers_ownership(arg0) {
            ctx.emit_owned_result_inc(arg_op, arg_ty);
        }
        let boxed = ctx.box_to_any(arg_op);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.promise_resolve_any, vec![boxed]),
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
    //
    // A view settles as an owned copy: the cell outlives whatever the
    // view borrows from (a split element's storage is its array's).
    let (arg_op, arg_ty, minted) = if arg_ty == Type::Substr {
        let owned = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.substr_to_owned, vec![arg_op]),
            Type::Str,
            None,
        );
        if ctx.expr_transfers_ownership(arg0) {
            ctx.emit_drop_value(arg_op, Type::Substr);
        }
        (Operand::Value(owned), Type::Str, true)
    } else {
        (arg_op, arg_ty, false)
    };
    if is_heap && !minted && !ctx.expr_transfers_ownership(arg0) {
        ctx.emit_owned_result_inc(arg_op, arg_ty);
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
    let repr =
        crate::ssa_lower_promise_repr_mark::promise_value_repr(&arg_ty, stored_as_f64, is_null);
    mint_settled(ctx, fid, method, arg_i64, is_heap, repr)
}

/// Allocates the settled cell; a fulfilled one with a known repr takes
/// it at allocation, any other is stamped after.
fn mint_settled(
    ctx: &mut LowerCtx<'_>,
    fid: FuncId,
    method: &str,
    arg_i64: Operand,
    is_heap: bool,
    repr: Option<i64>,
) -> Operand {
    if let Some(repr) = repr
        && method == "resolve"
    {
        return ctx.emit_promise_alloc_fulfilled_repr(arg_i64, is_heap, repr);
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arg_i64]),
        Type::Promise,
        None,
    );
    let out = Operand::Value(v);
    if let Some(repr) = repr {
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
