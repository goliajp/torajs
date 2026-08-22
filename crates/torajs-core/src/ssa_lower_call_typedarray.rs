//! `new <TypedArray>(…)` lowering per §23.2.5.1 —
//! RFC 20260823-typedarray-substrate 刀 2.
//!
//! Eleven constructor names share one kernel; the name resolves to a
//! [`Kind`] discriminant at compile time and rides in as a constant,
//! so the runtime never parses a name.
//!
//! Three argument slots, all BORROWED `any`. That is one more than
//! the `(target, handler)` pair helper offers, so this file lowers
//! its own — the shapes are otherwise identical, and a missing slot
//! is `undefined`, which the kernel reads as "not given".

use torajs_buffer::typedarray::Kind;

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The discriminant behind a constructor name, or `None` when the
/// name is not one of the eleven.
pub(crate) fn kind_of_name(name: &str) -> Option<i64> {
    Kind::from_name(name).map(|k| k as i64)
}

/// `BYTES_PER_ELEMENT` read off the CONSTRUCTOR (§23.2.5.1 table
/// 71) — a compile-time constant, since the name decides it.
pub(crate) fn bytes_per_element(name: &str) -> Option<i64> {
    Kind::from_name(name).map(Kind::element_size)
}

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, kind: i64, args: &[ExprId]) -> Operand {
    let ops = lower_borrowed_any_triple(ctx, args);
    let mut argv = vec![Operand::ConstI64(kind)];
    argv.extend(ops.iter().map(|(op, _, _)| op.clone()));
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.typedarray_create, argv),
        Type::Any,
        None,
    );
    crate::ssa_lower_new::release_borrowed_any_pair(ctx, ops);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// Three BORROWED-`any` slots, the same per-slot shape
/// `ssa_lower_new::lower_borrowed_any_pair` builds for two. Kept
/// here rather than generalised over a count because the pair form
/// documents its own reason for the object-literal lane, and this
/// one has no literal to worry about — a typed array's arguments are
/// a buffer and two numbers.
fn lower_borrowed_any_triple(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Vec<(Operand, bool, Option<ExprId>)> {
    let mut ops: Vec<(Operand, bool, Option<ExprId>)> = Vec::with_capacity(3);
    for slot in 0..3usize {
        let Some(&eid) = args.get(slot) else {
            // `undefined` is the answer for a slot the call did not
            // give, and every one of the kernel's three reads it
            // that way.
            let undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            ops.push((Operand::Value(undef), false, None));
            continue;
        };
        let raw = ctx.lower_expr(eid);
        let ty = ctx.operand_ty(&raw);
        if matches!(ty, Type::Any) {
            ops.push((raw, false, Some(eid)));
            continue;
        }
        if !ctx.expr_transfers_ownership(eid) && ty.is_refcounted() {
            ctx.emit_rc_inc(raw.clone());
        }
        ops.push((ctx.box_to_any(raw), true, None));
    }
    for &a in args.iter().skip(3) {
        let _ = ctx.lower_expr(a);
    }
    ops
}
