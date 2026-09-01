//! S-NEW 刀 2 — lowering for `Expr::NewDynamic`, the `new` whose
//! constructor is an expression.
//!
//! The named form is bound to a `__new_<C>` factory at compile time
//! and calls it directly. Here the callee has to be evaluated first,
//! so the shape is the ordinary indirect one: box the callee, pack the
//! arguments into the same argv buffer any-receiver method calls use,
//! and let `__torajs_anyv_construct` decide whether the value is a
//! constructor at all.
//!
//! Argument ledger follows the any-method-call arms: the runtime only
//! borrows argv and the callee, so every box minted here is released
//! after the call and before the throw check, while the returned
//! object arrives owned.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_any_method_call::pack_any_argv;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    // An argument list still carrying a dynamic `...spread` needs a
    // RUNTIME argc (§13.3.8.1 ArgumentListEvaluation), which the
    // fixed argv pack below cannot express — same claim-first rule
    // as the call cascade's spread lane. Evaluation order per
    // §13.3.5.1: the constructor expression first, then the
    // argument list left-to-right.
    if args
        .iter()
        .any(|a| matches!(ctx.ast.get_expr(*a), Expr::Spread { .. }))
    {
        return lower_spread(ctx, callee, args);
    }
    // Chunk-496 three-shape rule: a borrowed refcounted operand
    // rc-incs before the TRANSFER-shaped box, temps hand their
    // reference over, already-Any callees pass through borrowed.
    let callee_is_borrow = matches!(
        ctx.ast.get_expr(callee),
        Expr::Ident(_) | Expr::Member { .. }
    );
    let raw = ctx.lower_expr(callee);
    let raw_ty = ctx.operand_ty(&raw);
    let (target, target_boxed) = if raw_ty == Type::Any {
        (raw, false)
    } else {
        if callee_is_borrow && raw_ty.is_refcounted() {
            ctx.emit_rc_inc(raw.clone());
        }
        (ctx.box_to_any_from_expr(callee, raw), true)
    };

    // Rotation 550 — the target we hold (our box, or an owned temp
    // released below) is live across the arguments' lowers; park it.
    let target_tok = if target_boxed {
        Some(ctx.push_throw_temp(target.clone(), Type::Any))
    } else {
        ctx.park_owned_temp(callee, &target)
    };
    let packed = pack_any_argv(ctx, args);
    let argv = packed.argv;
    let cur_block = ctx.cur_block;
    let construct = ctx.intrinsics.construct;
    let result = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            construct,
            vec![
                target.clone(),
                Operand::Value(argv),
                Operand::ConstI64(args.len() as i64),
            ],
        ),
        Type::Any,
        None,
    );

    packed.release(ctx);
    ctx.unpark_owned_temp(target_tok);
    if target_boxed {
        ctx.emit_drop_value(target, Type::Any);
    } else {
        ctx.release_owned_temp(callee, &target);
    }
    // The constructed object is owned already, so the throw path has
    // to release it.
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}

/// `new callee(a, ...xs)` — materialize the full argument list into
/// one `Array<Any>` (the call cascade's `build_args_arr`, iteration
/// protocol included) and enter `__torajs_anyv_construct_spread`,
/// which reads argc off the array and runs the fixed-argc construct
/// path. Ledger follows `lower_bare_spread`: the Any box of the
/// callee is a BORROW (no rc transfer), the runtime borrows callee
/// and args, the args array drops after the call (releasing the
/// elements), an owned callee temp releases, and the constructed
/// object arrives owned for the throw path to account.
fn lower_spread(ctx: &mut LowerCtx<'_>, callee: ExprId, args: &[ExprId]) -> Operand {
    let raw = ctx.lower_expr(callee);
    let target = if matches!(ctx.operand_ty(&raw), Type::Any) {
        raw.clone()
    } else {
        ctx.box_to_any(raw.clone())
    };
    let args_arr = crate::ssa_lower_call_spread::build_args_arr(ctx, args);
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.anyv_construct_spread,
            vec![target, args_arr.clone()],
        ),
        Type::Any,
        None,
    );
    ctx.emit_drop_value(args_arr, Type::Arr(arr_any_id));
    ctx.release_owned_temp(callee, &raw);
    ctx.emit_throw_check_owned(None, Operand::Value(result), Type::Any);
    Operand::Value(result)
}
