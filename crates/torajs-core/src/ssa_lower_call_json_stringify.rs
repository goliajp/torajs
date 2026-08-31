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
//! - **S311** — `replacer` (args[1]) and `space` (args[2]) are
//!   evaluated left-to-right per ES §25.5.2 even when they turn out
//!   to be inert, so the loop below lowers every trailing argument
//!   for its side effects. A `space` becomes a gap Str the static
//!   unfold splices; a callable (or `any`) `replacer` takes the call
//!   OUT of the static unfold entirely — see `serves_as_replacer`.
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
    // Rotation 543 — both walks below READ the value they serialize;
    // neither consumes it. Only the step-12 sentinel arm released its
    // argument temp, so `JSON.stringify({a: 1})` (14.57 MB over 200k
    // churn) and `JSON.stringify([1, 2])` (27.20 MB) had no release
    // site, against 1.72 MB for the same loop over a bound receiver.
    // A number / string argument self-gates out (Copy, and a string
    // literal is not an owned shape), which is why only the composite
    // spellings leaked.
    let arg_raw = arg_op.clone();
    let arg_ty = ctx.operand_ty(&arg_op);
    // The checker's type for the argument rides along: SSA folds
    // `undefined` and `null` into one pointer-shaped slot, while
    // §25.5.2.4 omits an undefined property and prints a null one.
    // The walk peels this in step with its own shape recursion.
    let arg_fe = ctx.expr_types.get(&args[0]).cloned();
    let mut space_op = None;
    let mut replacer_op = None;
    // Rotation 543 — slots 1 and 2 are read the same way slot 0 is:
    // the box is rc-neutral and the kernel only borrows. Keep the
    // PRE-box operand and its ExprId so the release sees the
    // replacer's own `Type::Closure` — a per-call arrow mints a fresh
    // env block, which `release_owned_temp` recognises only through
    // `expr_minted_closure`, i.e. only before the any box.
    let mut replacer_raw: Option<(ExprId, Operand)> = None;
    let mut space_raw: Option<(ExprId, Operand)> = None;
    for (n, &a) in args.iter().enumerate().skip(1) {
        let op = ctx.lower_expr(a);
        if n == 1 && serves_as_replacer(ctx, a) {
            replacer_raw = Some((a, op.clone()));
            replacer_op = Some(ctx.box_to_any_from_expr(a, op.clone()));
        }
        if n == 2 {
            let ty = ctx.operand_ty(&op);
            // A pointer-shaped `space` is the `null` / `undefined`
            // spelling of "no indent" — step 8 leaves the gap empty,
            // so the static lane already answers byte-identically.
            if !matches!(ty, Type::Ptr) {
                space_raw = Some((a, op.clone()));
                space_op = Some((op, ty));
            }
        }
    }
    // The static unfold keeps its own frontend types, so it takes
    // the gap as a Str it splices into its concat chain. Computing it
    // is a single call at the call site, and every indent emission
    // downstream is behind `Option::is_some` — the compact form's
    // instruction sequence is untouched.
    let gap = space_op.map(|(space, space_ty)| {
        let boxed = if matches!(space_ty, Type::Any) {
            space
        } else {
            ctx.box_to_any(space)
        };
        let g = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_json_gap_str, vec![boxed]),
            Type::Str,
            None,
        );
        Operand::Value(g)
    });
    // A replacer can substitute ANY node's value at run time, so the
    // shape the static unfold monomorphizes against stops being known
    // — the whole walk moves into the kernel, with the value boxed
    // into the any lane the kernel serializes.
    if let Some(rep) = replacer_op {
        let boxed = ctx.box_to_any_from_expr(args[0], arg_op);
        let gap_arg = gap.clone().unwrap_or(Operand::ConstPtrNull);
        let out = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.json_stringify_full,
                vec![boxed, rep, gap_arg, Operand::ConstI64(0)],
            ),
            Type::Str,
            None,
        );
        if let Some(g) = gap {
            ctx.emit_drop_value(g, Type::Str);
        }
        ctx.emit_throw_check(None);
        ctx.release_owned_temp(args[0], &arg_raw);
        release_extra_args(ctx, &replacer_raw, &space_raw);
        return Some(Operand::Value(out));
    }
    // §25.5.2 step 12 — a top-level `undefined` or callable
    // serializes to NOTHING, so the CALL answers the undefined value
    // rather than any text. SSA folds `undefined` into the same Ptr
    // slot as `null`, which does print "null", so the verdict rides
    // down from the checker's type the way the composite arms take
    // it; a Closure has no static walk at all and used to be a loud
    // reject. Deliberately AFTER the replacer branch: with a function
    // replacer the root is still offered to it and can come back as
    // anything (`JSON.stringify(undefined, () => 42)` is "42").
    if (matches!(arg_fe, Some(crate::check::Type::Undefined))
        || matches!(arg_ty, Type::Closure(_) | Type::FnSig(_)))
        && let Some(sentinel) = ctx.str_undef_sentinel_for(Type::Str)
    {
        ctx.release_owned_temp(args[0], &arg_op);
        release_extra_args(ctx, &replacer_raw, &space_raw);
        if let Some(g) = gap {
            ctx.emit_drop_value(g, Type::Str);
        }
        return Some(sentinel);
    }
    let out = crate::ssa_lower_json_stringify::lower_top(ctx, arg_op, arg_ty, arg_fe, gap.clone());
    if let Some(g) = gap {
        ctx.emit_drop_value(g, Type::Str);
    }
    ctx.release_owned_temp(args[0], &arg_raw);
    release_extra_args(ctx, &replacer_raw, &space_raw);
    Some(out)
}

/// Settle the owned temps handed to slots 1 and 2. A per-call arrow
/// replacer (`JSON.stringify(o, (k, v) => v)`) mints a fresh closure
/// env every iteration and was the last leak left in this lane: 200k
/// churn at 14.78 MB RSS against 1.93 MB with the same arrow hoisted
/// into a binding.
fn release_extra_args(
    ctx: &mut LowerCtx<'_>,
    replacer_raw: &Option<(ExprId, Operand)>,
    space_raw: &Option<(ExprId, Operand)>,
) {
    for (eid, op) in [replacer_raw, space_raw].into_iter().flatten() {
        ctx.release_owned_temp(*eid, op);
    }
}

/// `true` when slot 2's checked type is one §25.5.2 step 4 would
/// actually consult — a callable (step 4.a), an array (step 4.b's
/// PropertyList), or an `Any` whose run-time shape only the kernel
/// can test. Everything else (a string, a number, `null`,
/// `undefined`) the spec discards outright, so the static unfold
/// keeps serving it and its instruction sequence is untouched.
fn serves_as_replacer(ctx: &LowerCtx<'_>, a: ExprId) -> bool {
    matches!(
        ctx.expr_types.get(&a),
        Some(
            crate::check::Type::Function(_, _)
                | crate::check::Type::Array(_)
                | crate::check::Type::Any
        )
    )
}
