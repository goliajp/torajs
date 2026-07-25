//! `JSON.stringify(value, replacer?, space?)` — recursive type-aware
//! serializer trampoline pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-55 of the decomp (chunks 1-54 = ... + dead JSON.stringify
//! 1-arg arm removal).
//!
//! Each call site is monomorphized inline by the receiver method
//! `lower_json_stringify` based on the static SSA type of `value`:
//! primitives → direct formatter, strings → quote helper, arrays /
//! structs → loop / static unfold + `__torajs_str_concat` chain.
//! No GC, single linear sweep.
//!
//! - **S311** — lower-and-drop `replacer` (args[1]) + `space`
//!   (args[2]) per ES §25.5.2. Spec evaluates them left-to-right;
//!   tora's stringify currently ignores them (replacer / space
//!   substrate is L3b). `check.rs:5772` already handled the
//!   typecheck-and-drop side (S272 idiom); the ssa mirror loop here
//!   so step()-style side-effect exprs still fire.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `JSON.stringify` Member-Ident shape, or args empty — the
//! typechecker upstream rejects the 0-arg form so the empty-args
//! guard is defensive).

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    if args.is_empty() {
        return None;
    }
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    if m_name != "stringify" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "JSON" {
        return None;
    }
    let arg_op = ctx.lower_expr(args[0]);
    let arg_ty = ctx.operand_ty(&arg_op);
    // The checker's type for the argument rides along: SSA folds
    // `undefined` and `null` into one pointer-shaped slot, while
    // §25.5.2.4 omits an undefined property and prints a null one.
    // The walk peels this in step with its own shape recursion.
    let arg_fe = ctx.expr_types.get(&args[0]).cloned();
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    Some(crate::ssa_lower_json_stringify::lower_top(
        ctx, arg_op, arg_ty, arg_fe,
    ))
}
