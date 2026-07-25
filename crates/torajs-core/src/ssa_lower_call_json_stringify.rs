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
use crate::ssa::{InstKind, Operand, Type};
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
    let mut space_op = None;
    for (n, &a) in args.iter().enumerate().skip(1) {
        let op = ctx.lower_expr(a);
        if n == 2 {
            let ty = ctx.operand_ty(&op);
            // A pointer-shaped `space` is the `null` / `undefined`
            // spelling of "no indent" — step 8 leaves the gap empty,
            // so the static lane already answers byte-identically.
            if !matches!(ty, Type::Ptr) {
                space_op = Some((op, ty));
            }
        }
    }
    // §25.5.2.1 steps 5-8 — a `space` argument indents the output,
    // which only a composite can show, and the runtime walk is the
    // one entry that carries a gap.
    //
    // Only the any lane routes here. Handing a STATIC composite to
    // the runtime walk would mean boxing it first, and a boxed struct
    // no longer carries the frontend types that tell an `undefined`
    // field apart from a null one (the class-layout tag is derived
    // from the SSA type, which folds them) — the key would come back
    // as `"u":null` instead of being omitted. The static lane grows
    // its own gap support rather than trading that back.
    if let Some((space, space_ty)) = space_op
        && matches!(arg_ty, Type::Any)
    {
        let boxed = arg_op;
        let space = if matches!(space_ty, Type::Any) {
            space
        } else {
            ctx.box_to_any(space)
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.anyv_json_stringify_spaced,
                vec![boxed, space],
            ),
            Type::Str,
            None,
        );
        ctx.emit_throw_check(None);
        return Some(Operand::Value(v));
    }
    Some(crate::ssa_lower_json_stringify::lower_top(
        ctx, arg_op, arg_ty, arg_fe,
    ))
}
