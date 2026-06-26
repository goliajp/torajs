//! `Object.assign(target, ...sources)` namespace static method pulled
//! out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call` dispatch
//! as chunk-20 of the `Expr::Call` god-arm decomp (chunks 1-19 = Arr
//! higher-order + Map dispatch + Set dispatch + Arr.push + Number
//! instance methods + bare-name globals + Str regex methods + Number
//! namespace + Array.from + Arr predicate iter + Arr.flatMap +
//! Object.entries + fn-indirect + Number/String/Boolean coercion +
//! universal methods + closure-local + Object.values + Object.keys +
//! Object.getPrototypeOf).
//!
//! S127-3 — N sources per ES §20.1.2.1. Copy each source's fields into
//! the target left-to-right; last-source-wins emerges naturally from
//! the per-store drop-old dance (each Store goes through the same
//! Load+drop+Store shape, so an intermediate source's value is dropped
//! before the next source overwrites).
//!
//! tr requires target = `Type::Obj(struct)` and sources of compatible
//! struct layout; typecheck rejects loose target types (Type::Any
//! follows a different path via dynobj copy, not this arm).
//!
//! Field copy uses the standard refcount discipline:
//! - Plain Copy field (`number`, `bool`): load → store, no rc work
//! - Refcounted scalar (`string` / `bigint` / ...): load → emit_rc_inc
//!   on borrowed source → store. Target's old value dropped first.
//! - Refcounted `Type::Arr(_)`: deep-clone via `__torajs_arr_slice` +
//!   per-element `emit_arr_rc_inc_range` so target gets its own array
//!   independent of the source.
//!
//! Returns `Some(target_op)` (target ptr passed through for chaining,
//! per ES spec — `Object.assign(t, s)` returns `t`). `None` lets the
//! caller fall through when callee isn't `Object.assign` or args.is_empty().

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let (ns_id, m_name) = match ctx.ast.get_expr(callee) {
        Expr::Member { obj, name } => (*obj, name.clone()),
        _ => return None,
    };
    if m_name != "assign" {
        return None;
    }
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "Object" {
        return None;
    }
    if args.is_empty() {
        return None;
    }
    let target_op = ctx.lower_expr(args[0]);
    let target_ty = ctx.operand_ty(&target_op);
    let Type::Obj(sid) = target_ty else {
        panic!("ssa-lower: Object.assign target must be a struct, got {target_ty:?}");
    };
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    for src_eid in &args[1..] {
        let source_op = ctx.lower_expr(*src_eid);
        for (idx, (_fname, fty)) in layout.iter().enumerate() {
            let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
            // Drop target's old value first (if non-Copy) so any
            // refcounted field properly releases before being
            // overwritten.
            if !fty.is_copy() {
                let old = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(*fty, target_op, offset),
                    *fty,
                    None,
                );
                ctx.emit_drop_value(Operand::Value(old), *fty);
            }
            // Load source.field (borrow).
            let src_v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*fty, source_op, offset),
                *fty,
                None,
            );
            let to_store = if let Type::Arr(arr_id) = *fty {
                // Deep-clone via arr_slice + per-element rc_inc so
                // target gets its own array.
                let len = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(src_v), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                let cloned = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_slice,
                        vec![
                            Operand::Value(src_v),
                            Operand::ConstI64(0),
                            Operand::Value(len),
                        ],
                    ),
                    *fty,
                    None,
                );
                let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
                if elem_ty.is_refcounted() {
                    let cloned_len = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(cloned), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    ctx.emit_arr_rc_inc_range(
                        Operand::Value(cloned),
                        Operand::ConstI64(0),
                        Operand::Value(cloned_len),
                    );
                }
                Operand::Value(cloned)
            } else {
                if fty.is_refcounted() {
                    ctx.emit_rc_inc(Operand::Value(src_v));
                }
                Operand::Value(src_v)
            };
            ctx.f
                .append_void(ctx.cur_block, InstKind::Store(to_store, target_op, offset));
        }
    }
    Some(target_op)
}
