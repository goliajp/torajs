//! The this-first spread lane: `super(...xs)` / `new C(...xs)` into a
//! constructor or factory, whose head lowers positionally while the
//! argument list materializes into one `Array<Any>` that feeds the
//! count, the argv pointer and the unpacked parameters.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};

use super::{build_args_arr, unpack};

/// The head width of a rest constructor (`__cm_<C>__ctor`, 2) or of
/// its factory (`__new_<C>`, 0). A unit class's factory leads with
/// the evaluation it was called with, which is not an argument.
pub(super) fn rest_ctor_head(ctx: &LowerCtx<'_>, name: &str) -> Option<usize> {
    let head = if name.starts_with("__cm_") && name.ends_with("__ctor") {
        2
    } else if let Some(factory) = name.strip_prefix("__new_") {
        let class = factory.split_once("$$").map_or(factory, |(c, _)| c);
        if ctx.ast.eval_unit_classes.contains(class) {
            return None;
        }
        0
    } else {
        return None;
    };
    let rest = ctx.ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name: n, params, .. }
            if n == name && params.last().is_some_and(|p| p.is_rest))
    });
    rest.then_some(head)
}

/// `super(a, ...xs)` against a ctor on the argv face. The head the
/// desugar wrote (`__this`, `__new_target`) lowers positionally;
/// everything after it materializes into one `Array<Any>` whose len
/// is the argc and whose slot base is the argv — the head-less
/// lane's mechanism, offset by the head. `argv` is false for a
/// callee on the argc tier alone, which takes the count but no
/// pointer, and `argc` false for a rest constructor with no count
/// channel, which takes only its parameters.
pub(super) fn lower_this_first_argv_spread(
    ctx: &mut LowerCtx<'_>,
    name: &str,
    args: &[ExprId],
    head: usize,
    argc: bool,
    argv: bool,
) -> Option<Operand> {
    if args.len() < head {
        return None;
    }
    let target = *ctx.fn_table.get(name)?;
    // The declared parameters, unpacked off the list below (decided
    // before anything is emitted — a `None` here must leave no
    // half-lowered call).
    let skip = head + usize::from(argc) + usize::from(argv);
    let declared = unpack::declared(ctx, name, head, skip)?;
    let head_ops: Vec<Operand> = args[..head].iter().map(|a| ctx.lower_expr(*a)).collect();
    let arr_any_id = intern_arr_layout(ctx.arr_layouts, Type::Any);
    let args_arr = build_args_arr(ctx, &args[head..]);
    // Same pre-call check as the head-less lane: a parent that
    // declares and reads nothing cannot throw, so the post-call check
    // is elided for it — and `super(...gen())`'s own throw was lost
    // (the test262 call-spread-err-mult-err family).
    ctx.emit_throw_check_owned(None, args_arr, Type::Arr(arr_any_id));
    let mut call_args = head_ops;
    if argc {
        let n = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, args_arr, crate::ssa_lower::ARR_LEN_OFF),
            Type::I64,
            None,
        );
        call_args.push(Operand::Value(n));
    }
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
