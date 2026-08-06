//! The gate in front of a static struct unfold — RFC
//! 20260806-declared-field-redefine.
//!
//! Several surfaces read a statically typed class instance by unfolding
//! its layout's member list at compile time, which is the whole reason
//! they are fast: `JSON.stringify`, `Object.values`, `Object.entries`,
//! `Object.assign`. All of them are defined over
//! EnumerableOwnProperties, and the program can move that set at
//! RUNTIME in two ways a member list cannot carry — hiding a declared
//! member with `Object.defineProperty(c, "f", {enumerable: false})`,
//! or adding a key the layout never mentions.
//!
//! They are reconciled the way the array tier already reconciles the
//! same tension (`emit_index_integrity_guard` over
//! `FLAG_ARR_EXOTIC_INDEX`): one masked test on the header word the
//! instance carries anyway, and only a hit takes the slower, fully
//! general path — the any-lane runtime walk, which consults each
//! member's attributes and produces the same answer for every shape
//! both tiers can express.
//!
//! An instance that was never passed to `defineProperty` pays one
//! not-taken branch and reaches the same unfold as before.
//!
//! The header read dereferences the cell. Every caller here holds a
//! `Type::Obj` operand, which cannot be JS null — a nullable struct
//! slot types as `Nullable` and never reaches these unfolds. The one
//! caller whose receiver CAN be null (`JSON.stringify` over a
//! `Nullable<Obj>` field) stands the gate behind its own §25.5.2 null
//! check rather than re-deriving one here; standing in FRONT of that
//! check is what re-broke it once already.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// The universal heap header is `[refcount u32 | tag u16 | flags u16]`,
/// so flags bit N sits at bit 48+N of the I64 load.
const FLAGS_SHIFT: u32 = 48;

/// The test itself: a `Bool` that is true when THIS instance's own
/// property set is no longer what its layout says — a declared member
/// was redefined (`FLAG_OBJ_EXOTIC_FIELD`), or a key the layout never
/// mentions was added (`FLAG_OBJ_EXPANDO`). Two facts, one masked
/// compare, because an unfold is wrong in exactly the same way under
/// either.
///
/// Exposed on its own because not every caller is producing a value.
/// `Object.assign` copies from its source into a target it already
/// has, so it wants the branch without the join.
///
/// `cell` must be a live, non-NULL `Tag::Obj` operand — see the module
/// doc.
pub(crate) fn emit_exotic_field_test(ctx: &mut LowerCtx<'_>, cell: &Operand) -> Operand {
    let mask =
        ((torajs_rc::FLAG_OBJ_EXOTIC_FIELD | torajs_rc::FLAG_OBJ_EXPANDO) as i64) << FLAGS_SHIFT;
    let hdr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, cell.clone(), 0),
        Type::I64,
        None,
    );
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr),
            Operand::ConstI64(mask),
        ),
        Type::I64,
        None,
    );
    let exotic = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(exotic)
}

/// Emit the gate: `walk` when [`emit_exotic_field_test`] says so,
/// `unfold` otherwise. Both arms produce a `ty` value; the join goes
/// through an alloca so neither arm needs to know where the other
/// landed.
///
/// `cell` must be a live, non-NULL `Tag::Obj` operand — see the module
/// doc.
pub(crate) fn with_exotic_field_gate(
    ctx: &mut LowerCtx<'_>,
    cell: &Operand,
    ty: Type,
    walk: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
    unfold: impl FnOnce(&mut LowerCtx<'_>) -> Operand,
) -> Operand {
    let exotic = emit_exotic_field_test(ctx, cell);
    let out_slot = ctx.alloca(ty.clone(), None);
    let walk_blk = ctx.f.add_block();
    let unfold_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: exotic,
            then_blk: walk_blk,
            else_blk: unfold_blk,
        },
    );

    // Redefined — the general walk, which reads each member's live
    // attributes rather than assuming the layout's.
    ctx.cur_block = walk_blk;
    let walked = walk(ctx);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(walked, Operand::Value(out_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done_blk));

    // The ordinary instance — the unfold, untouched.
    ctx.cur_block = unfold_blk;
    let unfolded = unfold(ctx);
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(unfolded, Operand::Value(out_slot), 0));
    ctx.f.set_term(blk, Terminator::Br(done_blk));

    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        done_blk,
        InstKind::Load(ty.clone(), Operand::Value(out_slot), 0),
        ty,
        None,
    );
    Operand::Value(v)
}
