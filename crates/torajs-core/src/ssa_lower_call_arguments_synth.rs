//! The `arguments`-object synthesis calls
//! (`__torajs_arguments_materialize` / `__torajs_arguments_mark`) —
//! split out of [`crate::ssa_lower_call_class_synth`] (file-size HARD
//! RULE) when the §20.5.1.1 call-constructs mark pushed the parent
//! past 500.
//!
//! They had been lodging there: the parent's own doc says "Four arms
//! share the same shape", and these two are not among the four —
//! they register no class and resolve no class name. The parent
//! answers "how a class is told to the runtime"; this answers "how
//! the arguments object is minted and stamped".

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `__torajs_arguments_materialize(__torajs_argv, <argc>)`
/// call (desugar_arguments_object prepends it to full-arguments
/// closure bodies) into `arr_alloc_any(argc)` + `arr_any_push(arr,
/// argv, argc, NULL)`. cap == argc so the push never relocates; the
/// runtime incs every stored heap cell (argv slots stay borrowed by
/// the adapter's caller). Result is the fresh Arr<Any> the
/// `__torajs_arguments: any[]` binding owns (scope drop reclaims).
pub(crate) fn try_lower_materialize(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let argv = ctx.lower_expr(args[0]);
    let argc = ctx.lower_expr(args[1]);
    let arr_id = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Any);
    let arr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![argc]),
        Type::Arr(arr_id),
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_any_push,
            vec![Operand::Value(arr), argv, argc, Operand::ConstPtrNull],
        ),
    );
    Some(Operand::Value(arr))
}

/// `__torajs_arguments_mark(__torajs_arguments)` — the
/// FLAG_ARR_ARGUMENTS stamp statement both desugar lanes append
/// right after the mint (§10.4.4 arguments-exotic `"length"`
/// attributes; the keyed readers gate on the bit). The operand is
/// the binding's Arr pointer — a borrowed read, no rc movement.
pub(crate) fn try_lower_mark(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let arr = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_mark_arguments, vec![arr]),
    );
    Some(Operand::ConstI64(0))
}
