//! `Expr::PostIncr { target, is_inc, is_prefix }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-74
//! of the decomp (chunks 1-73 = ... + `Expr::Nullish` 4-layer
//! dispatch).
//!
//! JS spec: yield the OLD value, then mutate. Three target shapes
//! mirror Assign:
//!
//! - **Ident** (`x++`) — `load slot → compute new → store new`.
//!   V3-18 m1.h.26 global slot path: when the target ident is a
//!   registered mutable global (e.g. a static class field after
//!   the ClassDecl desugar), `GlobalRef + Load + Store` matches
//!   the read / Assign paths and keeps post-incr working for
//!   `Counter.value++`.
//! - **Member** (`obj.field++`) — `load obj+offset → compute new
//!   → store new at same offset` via struct layout walk. The
//!   `xs.length++/--` array form routes the store through the
//!   §10.4.2.5 resize helper instead (see [`crate::ssa_lower_post_incr_len::lower_arr_length`]).
//! - **Index** (`arr[i]++`) — `load arr+offset → compute new →
//!   store new at same addr` via T-13.5 head-aware
//!   `emit_arr_slot_byte_offset` (11-A1 non-deque peek fast
//!   path).
//!
//! On the typed lanes the incr op must match the slot width —
//! `FAdd`/`FSub` on `ConstF64(1.0)` for f64 slots, `Add`/`Sub` on
//! `ConstI64(1)` for i64 (codegen dispatches register class by op
//! kind, so an int Add with an f64 result is an invalid SSA shape).
//!
//! An `any` slot cannot be stepped inline at all: §13.4.4.1 puts a
//! ToNumeric between the load and the add, which must run exactly once
//! and which picks the numeric domain — BigInt or Number — the step
//! happens in. Both Ident shapes hand the slot pointer to one runtime
//! call instead ([`lower_any_slot`]). Member on an `any` RECEIVER has
//! no slot pointer to hand over — the store must go back through the
//! member-set kernel with its accessor / refusal semantics — so
//! [`lower_member_any`] composes three existing lanes: the
//! accessor-aware any-member read (GetV), the value-shaped
//! ToNumeric + step kernel (`anyv_incr_value`), and the tag-gated
//! member-set tail. Index on an `any` element stays loud (registered).
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::PostIncr` match arm bottoms out here).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};
use crate::ssa_lower_post_incr_len::incr_op_one;

/// `eid` is the `Expr::PostIncr` node. `is_prefix` picks the answer:
/// the postfix form yields the coerced OLD value, the prefix form
/// (`++x`, rotation 626) the NEW one — same ToNumeric, same step,
/// same store on every lane.
pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId) -> Operand {
    let Expr::PostIncr {
        target,
        is_inc,
        is_prefix,
    } = *ctx.ast.get_expr(eid)
    else {
        panic!("ssa-lower: post-incr lowering entered on a non-update node");
    };
    match ctx.ast.get_expr(target).clone() {
        // RFC 20260730-undeclared-ident, write position — §13.4.4.1
        // step 1 is GetValue on the target, so an update expression
        // over a marked unresolvable name raises the read-side
        // ReferenceError before any numeric step; nothing else here
        // is reachable.
        Expr::Ident(name) if ctx.ast.undeclared_reads.contains_key(&target) => {
            // …unless the global object carries the name. §13.4.4.1
            // step 1 is GetValue, which under the sloppy goal consults
            // the object half of the global environment record before
            // it throws (RFC 20260911-global-object-record) — and a
            // `for (i = 0; …; i++)` whose `i` the first clause just
            // created is exactly that shape.
            if crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx)
                && !ctx.ast.strict_eval_assign_names.contains(&name)
            {
                return lower_global_ident_update(ctx, &name, Step { is_inc, is_prefix });
            }
            let name_str = ctx.intern_string_literal(&name);
            let raiser = ctx.intrinsics.throw_reference_error_name;
            let cur_block = ctx.cur_block;
            ctx.f.append_void(
                cur_block,
                InstKind::Call(raiser, vec![Operand::Value(name_str)]),
            );
            ctx.emit_throw_check(None);
            Operand::ConstPtrNull
        }
        Expr::Ident(name) => {
            let v = lower_ident(ctx, eid, name.clone(), Step { is_inc, is_prefix });
            ctx.proofs.kill(&name);
            v
        }
        Expr::Member { obj, name: field } => {
            lower_member(ctx, eid, target, obj, field, Step { is_inc, is_prefix })
        }
        Expr::Index { obj, index } => lower_index(ctx, eid, obj, index, Step { is_inc, is_prefix }),
        other => panic!("ssa-lower: post-incr target shape not supported: {other:?}"),
    }
}

/// Box the receiver and step `field` through the any-member lane. No
/// write-back and no release: the receiver's own slot holds its pointer,
/// not an `AnyValue`.
fn boxed_member_step(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    obj_val: Operand,
    field: &str,
    step: Step,
) -> Operand {
    let boxed = ctx.box_to_any(obj_val);
    crate::ssa_lower_post_incr_any::lower_member_any(
        ctx, eid, target, None, false, boxed, field, step,
    )
}

/// §13.4.4 over the global object: read it, ToNumber, write it back,
/// and answer the old or the new value. The read throws if the object
/// does not carry the name — which is what §13.4.4.1 step 1 wants,
/// since GetValue runs before any numeric step.
fn lower_global_ident_update(ctx: &mut LowerCtx<'_>, name: &str, step: Step) -> Operand {
    let name_str = ctx.intern_string_literal(name);
    let getter = ctx.intrinsics.global_ident_get;
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Call(getter, vec![Operand::Value(name_str)]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    let conv = ctx.intrinsics.any_to_number;
    let cur_block = ctx.cur_block;
    let old_num = Operand::Value(ctx.f.append_inst(
        cur_block,
        InstKind::Call(conv, vec![Operand::Value(old)]),
        Type::F64,
        None,
    ));
    let (one, op) = incr_op_one(Type::F64, step.is_inc);
    let cur_block = ctx.cur_block;
    let new = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, old_num, one),
        Type::F64,
        None,
    );
    let boxed = ctx.box_to_any(Operand::Value(new));
    let name_str = ctx.intern_string_literal(name);
    let setter = ctx.intrinsics.global_ident_set;
    let cur_block = ctx.cur_block;
    let stored = ctx.f.append_inst(
        cur_block,
        InstKind::Call(setter, vec![Operand::Value(name_str), boxed]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    if step.is_prefix {
        Operand::Value(stored)
    } else {
        old_num
    }
}

/// One update's two switches, carried together so the lanes take one
/// argument instead of two bare bools.
#[derive(Clone, Copy)]
pub(crate) struct Step {
    pub(crate) is_inc: bool,
    pub(crate) is_prefix: bool,
}

fn lower_ident(ctx: &mut LowerCtx<'_>, eid: ExprId, name: String, step: Step) -> Operand {
    if !ctx.locals.contains_key(&name)
        && let Some(slot_ty) = ctx.globals.get(&name).copied()
    {
        return lower_ident_global(ctx, eid, name, slot_ty, step);
    }
    let info = match ctx.locals.get(&name) {
        Some(i) => *i,
        None => panic!("ssa-lower: post-incr on unknown ident `{name}`"),
    };
    if info.ty == Type::Any {
        return crate::ssa_lower_post_incr_any::lower_any_slot(
            ctx,
            eid,
            Operand::Value(info.slot),
            step,
        );
    }
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
        info.ty,
        None,
    );
    let (one, op) = incr_op_one(info.ty, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        info.ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), Operand::Value(info.slot), 0),
    );
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        Operand::Value(old)
    }
}

fn lower_ident_global(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    name: String,
    slot_ty: Type,
    step: Step,
) -> Operand {
    let cur_block = ctx.cur_block;
    let ptr = ctx
        .f
        .append_inst(cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
    if slot_ty == Type::Any {
        return crate::ssa_lower_post_incr_any::lower_any_slot(ctx, eid, Operand::Value(ptr), step);
    }
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::Load(slot_ty, Operand::Value(ptr), 0),
        slot_ty,
        None,
    );
    let (one, op) = incr_op_one(slot_ty, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        slot_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::Store(Operand::Value(new_v), Operand::Value(ptr), 0),
    );
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        Operand::Value(old)
    }
}

fn lower_member(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    target: ExprId,
    obj: ExprId,
    field: String,
    step: Step,
) -> Operand {
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);
    // §13.4.2.1 — the old value is GetValue(lhs), which is §7.1.20
    // ToObject on a possibly-absent receiver: the TypeError comes
    // before any load. The any lane's kernel raises it on its own.
    if !matches!(obj_ty, Type::Any) {
        crate::ssa_lower_nullable_guard_emit::emit_undefable_heap_guard(ctx, obj, &obj_val);
    }
    // Length-write knife (rotation 270) — `xs.length--` / `++` on an
    // array: load the len, step, route the store through the
    // §10.4.2.5 resize helper (mirrors `lower_arr_length_assign`;
    // truncation releases slots, grow fills holes), yield the old
    // value. Surfaced by LiveLength arguments bodies decrementing
    // `arguments.length`, but serves plain arrays the same.
    if matches!(obj_ty, Type::Arr(_)) && field == "length" {
        return crate::ssa_lower_post_incr_len::lower_arr_length(ctx, obj, obj_val, obj_ty, step);
    }
    if matches!(obj_ty, Type::Any) {
        let obj_ident = match ctx.ast.get_expr(obj) {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        };
        let recv_owned = ctx.expr_transfers_ownership(obj);
        return crate::ssa_lower_post_incr_any::lower_member_any(
            ctx, eid, target, obj_ident, recv_owned, obj_val, &field, step,
        );
    }
    let sid = match obj_ty {
        Type::Obj(sid) => sid,
        // Everything else: box the cell and take the any lane, the
        // same last lane the STORE ladder grew in rotation 527's
        // write-side twin (`ssa_lower_assign_member`). Knowing the
        // receiver's shape is not a licence to skip §13.4.4.1 — and
        // this arm was a hard `panic!`, so a specialized body's
        // `n.hit++` on a primitive answered "not yet supported:
        // post-incr field on non-obj I64" while the very same program
        // spelled `(n as any).hit++` ran and raised the TypeError
        // §10.1.8.1 owes it.
        //
        // No write-back and no release ride this tail: a typed cell's
        // slot holds its own pointer, not an `AnyValue`, so handing
        // the set lane an ident to re-store would corrupt it — which
        // is why `obj_ident` and `recv_owned` are now the caller's to
        // supply rather than derived inside the lane.
        _ => return boxed_member_step(ctx, eid, target, obj_val, &field, step),
    };
    let slot = ctx.struct_layouts[sid.0 as usize]
        .iter()
        .position(|(fname, _)| fname == &field);
    let Some(idx) = slot else {
        // a name the layout never carries (a class accessor, an expando)
        // steps through the any lane, whose read and write tails own the
        // accessor dispatch, as the assignment's miss arm does; a private
        // method stays refused
        assert!(
            !field.starts_with("__priv_"),
            "ssa-lower: struct {sid:?} has no field `{field}`"
        );
        return boxed_member_step(ctx, eid, target, obj_val, &field, step);
    };
    let field_ty = ctx.struct_layouts[sid.0 as usize][idx].1;
    // a cell the program reaches as `any` (`(p as any).k++`), or a field
    // a write of the program can shadow, steps through the any lane: the
    // slot may not hold the field's value
    use crate::ssa_lower_member_obj_field::shadow_read;
    if matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::Any))
        || shadow_read::applies(ctx, Some(obj), &field, field_ty)
        || shadow_read::mistyped(ctx, target, &field, field_ty)
    {
        return boxed_member_step(ctx, eid, target, obj_val, &field, step);
    }
    let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
    let guarded = crate::ssa_lower_member_obj_field::exotic_read::guarded_load(
        ctx,
        eid,
        obj,
        obj_val,
        (offset, field_ty),
        &field,
    );
    let old = guarded.unwrap_or_else(|| {
        let cur_block = ctx.cur_block;
        Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::Load(field_ty, obj_val, offset),
            field_ty,
            None,
        ))
    });
    let (one, op) = incr_op_one(field_ty, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx
        .f
        .append_inst(cur_block, InstKind::BinOp(op, old, one), field_ty, None);
    // `o.k++` ends in PutValue (§13.4.4.1 step 5), so it owes the
    // §10.1.9 writable gate the same as `o.k = v` does — the read and
    // the ToNumeric step come first, then the store may raise. This
    // lane emitted a bare `Store` at the field offset instead, so a
    // FROZEN instance's `o.hit++` did not just swallow its TypeError:
    // it wrote through the freeze and answered "after", while the
    // plain-assign spelling of the same field threw. Same guard, one
    // definition — the `Store` below rides the block it leaves us on.
    use crate::ssa_lower_assign_member_field::guard;
    let done = guard::emit_field_writable_guard(ctx, obj, obj_val, &field, Operand::Value(new_v));
    let store_block = ctx.cur_block;
    ctx.f.append_void(
        store_block,
        InstKind::Store(Operand::Value(new_v), obj_val, offset),
    );
    guard::finish_guarded_store(ctx, done);
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        old
    }
}

fn lower_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    step: Step,
) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    let arr_ty = ctx.operand_ty(&arr_val);
    // Cluster #4 blade 2 — an `any` (or boxed-struct) receiver's
    // keyed update rides the keyed kernels, mirroring
    // [`lower_member_any`]. A null/undefined receiver raises inside
    // the keyed read.
    if matches!(arr_ty, Type::Any | Type::Obj(_)) {
        let recv = if matches!(arr_ty, Type::Obj(_)) {
            ctx.box_to_any(arr_val)
        } else {
            arr_val
        };
        return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
            ctx, eid, index, recv, step,
        );
    }
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        // A PRIMITIVE receiver: `n[0]++` is §13.4.4.1 over a value
        // with no properties, so it raises the TypeError §10.1.8.1
        // owes a strict-mode write — which is exactly what the keyed
        // any lane above already answers. Only the typed spelling was
        // refused, and only by the panic below: a specialized body's
        // `n[0]++` said "not yet supported: post-incr index on
        // non-array I64" while `const n: any = 1; n[0]++` threw.
        //
        // The other shapes this arm catches (Map, Set, Date, RegExp …)
        // deliberately keep the loud refusal. Asked one at a time
        // rather than assumed from the member ladder's list: the keyed
        // any lane does NOT increment a Map's expando —
        // `mm[0] = 5; mm[0]++` leaves it at 5 where bun answers 6 —
        // so boxing them here would trade a compile-time refusal for
        // a silent wrong answer, which is the trade rotation 608
        // declined on the read side. They are registered against that
        // lane instead.
        Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Str | Type::Substr => {
            let boxed = ctx.box_to_any(arr_val);
            return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
                ctx, eid, index, boxed, step,
            );
        }
        other => panic!("ssa-lower: post-incr index on non-array {other:?}"),
    };
    // An `any[]` element is a NaN-box: the raw add below would add to
    // its bits, and the unchecked slot store skips everything an
    // element write can mean (an index past the end, an accessor, an
    // arguments object's own index in its props bag). The keyed lane
    // does the §13.4 update through the element kernels.
    if elem_ty == Type::Any {
        let boxed = ctx.box_to_any(arr_val);
        return crate::ssa_lower_post_incr_keyed::lower_index_any_keyed(
            ctx, eid, index, boxed, step,
        );
    }
    let idx_val = ctx.lower_index_operand(index);
    let (offset_base, offset) = ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 3, is_non_deque);
    let cur_block = ctx.cur_block;
    let old = ctx.f.append_inst(
        cur_block,
        InstKind::LoadDyn(elem_ty, offset_base, offset),
        elem_ty,
        None,
    );
    let (one, op) = incr_op_one(elem_ty, step.is_inc);
    let cur_block = ctx.cur_block;
    let new_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(op, Operand::Value(old), one),
        elem_ty,
        None,
    );
    ctx.f.append_void(
        cur_block,
        InstKind::StoreDyn(Operand::Value(new_v), offset_base, offset),
    );
    if step.is_prefix {
        Operand::Value(new_v)
    } else {
        Operand::Value(old)
    }
}
