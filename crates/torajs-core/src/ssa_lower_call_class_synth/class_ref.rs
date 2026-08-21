//! `__torajs_my_class_ref("C")` — the factory-side `new.target`
//! materializer — split out of `ssa_lower_call_class_synth.rs` when
//! rotation 470's skip-when-unread gate pushed the parent over the
//! 500 line cap (the r458 watch, 余量 6, cashing in). The parent
//! keeps the synthetic-callee dispatch; this child answers one
//! question: what Any value does a ctor's `__new_target` slot get.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(super) fn try_lower_my_class_ref(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.clone();
    let cur_block = ctx.cur_block;
    // `__new_target` exists for `new.target` reads — the only reader
    // of its VALUE (super() forwarding passes it on unread; the
    // runtime-side `new` paths mint their own). A program with no
    // `new.target` anywhere never observes it, so hand the ctor
    // `undefined` and skip `class_get`: that call rc_inc'd the class
    // object, the ctor's scope exit rc_dec'd it, and that dec ran the
    // cycle-buffer probe — together ~24% of a method-call loop
    // (rotation 470). `Expr::NewTarget` survives desugar unchanged
    // (desugar_classes pass 2 deliberately leaves it), so the arena
    // scan is the whole-program census.
    let reads_new_target = ctx.ast.exprs.iter().any(|e| matches!(e, Expr::NewTarget));
    if let Some(tag) = ctx
        .class_name_to_tag
        .get(&cname)
        .copied()
        .filter(|_| reads_new_target)
    {
        let class_get = ctx.intrinsics.class_get;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(class_get, vec![Operand::ConstI64(tag as i64)]),
            Type::Any,
            None,
        );
        return Some(Operand::Value(v));
    }
    // No tag (or nobody reads new.target) → the ANY_UNDEF Any-box.
    let any_box = ctx.intrinsics.any_box;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(any_box, vec![Operand::ConstI64(5), Operand::ConstI64(0)]),
        Type::Any,
        None,
    );
    Some(Operand::Value(v))
}
