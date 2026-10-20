//! The argument of a pointer slot that may hold NULL or the
//! `undefined` cell: which lane serializes it, and the static unfold
//! behind the cell and NULL compares.

use crate::ast::ExprId;
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_nullable_guard::{holds_cell_unproven_only, holds_undef_cell};

/// Serialize the argument of `JSON.stringify` (no replacer, not the
/// step-12 sentinel) with its temp parked across the emitted code.
pub(super) fn lower_arg(
    ctx: &mut LowerCtx<'_>,
    arg: ExprId,
    arg_op: Operand,
    arg_ty: Type,
    arg_fe: Option<crate::check::Type>,
    gap: Option<Operand>,
) -> Operand {
    // A pointer-shaped slot the checker typed `T | undefined` holds
    // the generic immortal cell for its undefined, and the static
    // unfold below monomorphizes against the shape the slot COULD
    // hold — a premise that is false exactly when it is holding that
    // cell. So `const o: { a: number } | undefined = undefined;
    // JSON.stringify(o)` read the cell as a live struct and printed
    // `{"a":8752868413093667698}` — whatever the field offset landed
    // on. The array spelling answered an empty string; the class one
    // printed the same garbage. Every engine answers `undefined`
    // (§25.5.2 step 12).
    //
    // The step-12 arm in `try_lower` only fires on a statically `Undefined`
    // argument, and this slot is not that: at run time it is either
    // the cell or a live value, so the verdict cannot be made here.
    // The any lane already makes it at run time and is §25.5.2
    // itself — it answers the undefined VALUE for the cell and
    // serializes a live one exactly as the unfold would, gap
    // included. Hand it the boxed slot and take that walk.
    //
    // Self-gating on the checker's nullable typing: a slot that was
    // never nullable keeps the static unfold, so no program that
    // could not reach the bug pays for it.
    // A `toJSON` under the walk can throw at a check inside it: the
    // argument temp is parked across the walk so that edge releases
    // it too.
    //
    // a read that holds the cell only because the live-field census
    // could not prove it live had its static type trusted before the
    // census judged such reads: the cell and NULL are two compares,
    // and any other value it holds is one of that type, so it keeps
    // the unfold (`split_nullish`)
    let holds_cell = holds_undef_cell(ctx, arg, arg_ty);
    let split =
        holds_cell && unfolds_live_only(arg_ty) && holds_cell_unproven_only(ctx, arg, arg_ty);
    if holds_cell && !split {
        let boxed = ctx.box_to_any_from_expr(arg, arg_op);
        let parked = ctx.park_owned_temp(arg, &arg_op);
        let out = crate::ssa_lower_json_stringify::emit_any_walk(ctx, boxed, gap, 0);
        ctx.unpark_owned_temp(parked);
        return out;
    }
    let parked = ctx.park_owned_temp(arg, &arg_op);
    let unfold = |ctx: &mut LowerCtx<'_>| {
        crate::ssa_lower_json_stringify::lower_top(ctx, arg_op, arg_ty, arg_fe, gap)
    };
    let out = match split {
        true => split_nullish(ctx, arg_op, unfold),
        false => unfold(ctx),
    };
    ctx.unpark_owned_temp(parked);
    out
}

/// The pointer types the static unfold serializes from the value
/// alone; the others (a closure, a symbol, a BigInt, a bare pointer)
/// answer at run time through the any-lane walk.
fn unfolds_live_only(ty: Type) -> bool {
    matches!(
        ty,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Date
            | Type::RegExp
            | Type::Promise
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
    )
}

/// The argument of a pointer slot that may hold NULL or the
/// `undefined` cell: the cell answers the undefined value (§25.5.2
/// step 12), NULL the text `null`, and any other value is one of the
/// slot's static type, which `live` serializes.
fn split_nullish(
    ctx: &mut LowerCtx<'_>,
    val_op: Operand,
    live: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let out_slot = ctx.alloca_in_entry(Type::Str, Some("__json_top_out"));
    let ty = ctx.operand_ty(&val_op);
    let cell = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        ty,
        None,
    );
    let is_cell = ctx.cmp(IPred::Eq, val_op, Operand::Value(cell));
    let (cell_blk, rest_blk) = (ctx.f.add_block(), ctx.f.add_block());
    let (null_blk, live_blk, done_blk) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: is_cell,
            then_blk: cell_blk,
            else_blk: rest_blk,
        },
    );
    ctx.cur_block = rest_blk;
    let is_null = ctx.cmp(IPred::Eq, val_op, Operand::ConstPtrNull);
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: is_null,
            then_blk: null_blk,
            else_blk: live_blk,
        },
    );
    ctx.cur_block = cell_blk;
    let undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
        Type::Str,
        None,
    );
    store_and_join(ctx, Operand::Value(undef), out_slot, done_blk);
    ctx.cur_block = null_blk;
    let null = Operand::Value(ctx.intern_string_literal("null"));
    store_and_join(ctx, null, out_slot, done_blk);
    ctx.cur_block = live_blk;
    let v = live(ctx);
    store_and_join(ctx, v, out_slot, done_blk);
    ctx.cur_block = done_blk;
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(out)
}

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, slot: ValueId, join: BlockId) {
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
}
