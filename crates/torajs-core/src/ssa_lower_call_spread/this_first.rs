//! The this-first spread lane: `super(...xs)` / `new C(...xs)` into a
//! constructor or factory, whose head lowers positionally while the
//! argument list materializes into one `Array<Any>` that feeds the
//! count, the argv pointer and the unpacked parameters.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

use super::{build_args_arr, unpack};

/// `super(a, ...xs)` against a ctor on the argv face. The head the
/// desugar wrote (`__this`, `__new_target`) lowers positionally;
/// everything after it materializes into one `Array<Any>` whose len
/// is the argc and whose slot base is the argv — the head-less
/// lane's mechanism, offset by the head. `argv` is false for a
/// callee on the argc tier alone, which takes the count but no
/// pointer.
pub(super) fn lower_this_first_argv_spread(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    args: &[ExprId],
    head: usize,
    argv: bool,
) -> Option<Operand> {
    if args.len() < head {
        return None;
    }
    let target = *ctx.fn_table.get(name)?;
    // The declared parameters, unpacked off the list below (decided
    // before anything is emitted — a `None` here must leave no
    // half-lowered call).
    let declared = unpack::declared(ctx, name, head, head + 1 + usize::from(argv))?;
    let head_ops: Vec<Operand> = args[..head].iter().map(|a| ctx.lower_expr(*a)).collect();
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let args_arr = build_args_arr(ctx, &args[head..]);
    // Same pre-call check as the head-less lane: a parent that
    // declares and reads nothing cannot throw, so the post-call check
    // is elided for it — and `super(...gen())`'s own throw was lost
    // (the test262 call-spread-err-mult-err family).
    ctx.emit_throw_check_owned(None, args_arr, Type::Arr(arr_any_id));
    let argc = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, args_arr, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let mut call_args = head_ops;
    call_args.push(Operand::Value(argc));
    if argv {
        call_args.push(ctx.emit_arr_data_ptr(args_arr));
    }
    // The declared parameters, unpacked off the same list.
    // `[…head, argc, argv, …declared]` is the this-first argv
    // signature (`inject_argc_params`).
    let spread_eid = args[head..]
        .iter()
        .copied()
        .find(|a| matches!(ctx.ast.get_expr(*a), Expr::Spread { .. }))
        .expect("this-first argv spread lane entered without a spread argument");
    let mut unpacked = unpack::Unpacked::default();
    call_args.extend(unpack::emit(
        ctx,
        args_arr,
        spread_eid,
        declared,
        &mut unpacked,
    ));
    let ret_ty = ctx.f_ret_type_hint(target);
    let result = if ret_ty == Type::Void {
        ctx.f
            .append_void(ctx.cur_block, InstKind::Call(target, call_args));
        Operand::ConstPtrNull
    } else {
        Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, call_args),
            ret_ty,
            None,
        ))
    };
    unpack::release(ctx, unpacked);
    ctx.emit_drop_value(args_arr, Type::Arr(arr_any_id));
    ctx.emit_throw_check(Some(target));
    Some(result)
}
