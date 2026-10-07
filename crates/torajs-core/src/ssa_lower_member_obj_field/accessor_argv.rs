//! Direct accessor calls into a body on the runtime argv face.
//!
//! [[Get]] calls a getter with no arguments and [[Set]] calls a setter
//! with one (§10.1.8.1 OrdinaryGet, §10.1.9.2
//! OrdinarySetWithOwnDescriptor), but the same function object can be
//! taken out with `Object.getOwnPropertyDescriptor` and called with any
//! count, so an accessor body that reads `arguments` cannot fold the
//! count. The arguments pass gives it the argv face instead: the real
//! count rides the hidden argc slot and the values the `__torajs_argv`
//! pointer. The boxed adapter fills both for every call through the
//! function value; the direct accessor lanes fill them here.

use crate::ast::ExprId;
use crate::ssa::{BlockId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Box `args` into an entry-block `AnyValue[]` and answer its pointer.
/// The slots are borrows, as in the boxed adapter: the body's
/// arguments materialize takes its own stake in every heap cell, and
/// each operand outlives the call.
pub(crate) fn pack_argv(ctx: &mut LowerCtx<'_>, args: &[(Operand, Option<ExprId>)]) -> Operand {
    let buf = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes((args.len().max(1) * 8) as u64),
        Type::Ptr,
        Some("__acc_argv"),
    );
    for (i, &(op, eid)) in args.iter().enumerate() {
        let slot = match eid {
            _ if ctx.operand_ty(&op) == Type::Any => op,
            Some(e) => ctx.box_to_any_from_expr(e, op),
            None => ctx.box_to_any(op),
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot, Operand::Value(buf), (i * 8) as u64),
        );
    }
    Operand::Value(buf)
}

/// Did the arguments pass put this `__cm_` accessor on the method argv
/// face (params `[__this, __torajs_argv, …user]`, sig `[this, argc,
/// argv, …user]`)? A generic declarer's specialization copies the
/// declaration's params, so the declared name answers for it as well.
fn class_accessor_takes_argv(ctx: &LowerCtx<'_>, fn_name: &str) -> bool {
    ctx.ast.method_argv_fns.contains(fn_name)
}

/// The SSA type of a `__cm_` setter's value param (`fid` is the setter
/// or its specialization). The sig is receiver-first, and on the argv
/// face the argc / argv pair sits between the receiver and the value.
pub(crate) fn class_setter_value_ty(
    ctx: &LowerCtx<'_>,
    fid: crate::ssa::FuncId,
    fn_name: &str,
) -> Option<Type> {
    let at = if class_accessor_takes_argv(ctx, fn_name) {
        3
    } else {
        1
    };
    let sig = ctx.fn_sig_ids.get(&fid)?;
    ctx.fn_sigs[sig.0 as usize].0.get(at).copied()
}

/// The argument list of a direct call to a `__cm_` accessor: the
/// receiver, then on the argv face the count and the packed values,
/// then the user params.
pub(crate) fn class_accessor_call_args(
    ctx: &mut LowerCtx<'_>,
    fn_name: &str,
    recv: Operand,
    user: &[(Operand, Option<ExprId>)],
) -> Vec<Operand> {
    let mut args = vec![recv];
    if class_accessor_takes_argv(ctx, fn_name) {
        args.push(Operand::ConstI64(user.len() as i64));
        args.push(pack_argv(ctx, user));
    }
    args.extend(user.iter().map(|&(op, _)| op));
    args
}
