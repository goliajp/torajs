//! The typed store's leg for a cell whose object literal left a member
//! out (`torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD`), beside the frozen /
//! redefined-field leg of [`super`].

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// A cell whose literal left a member out never carries the other two
/// bits (`torajs_meta::struct_absent`), so its store needs no [[Set]]:
/// the slot's bit in the absent set goes, and the header bit with it
/// once the set is empty — the store creates the member when it was
/// the one left out. Continues `cur_block` on the exotic leg.
pub(super) fn clear_absent(
    ctx: &mut LowerCtx,
    (obj_val, masked): (Operand, Operand),
    field: &str,
    store_blk: BlockId,
) {
    use crate::ssa::BinOp;
    let flag = (u32::from(torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD) << 16) as i32;
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::And, masked, Operand::ConstI32(flag)),
        Type::I32,
        None,
    );
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let (clear_blk, exotic_blk) = (ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: clear_blk,
            else_blk: exotic_blk,
        },
    );
    ctx.cur_block = clear_blk;
    let slot = match ctx.operand_ty(&obj_val) {
        Type::Obj(sid) => ctx.struct_layouts[sid.0 as usize]
            .iter()
            .position(|(n, _)| *n == field)
            .filter(|i| *i < 32),
        _ => None,
    };
    if let Some(i) = slot {
        let bin = |ctx: &mut LowerCtx, op, a, b, ty| {
            let v = ctx
                .f
                .append_inst(ctx.cur_block, InstKind::BinOp(op, a, b), ty, None);
            Operand::Value(v)
        };
        // the set is the high half of the `+8` word, beside the class tag
        let word = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, obj_val, 8),
            Type::I64,
            None,
        );
        let mask = Operand::ConstI64(!(1i64 << (32 + i)));
        let rest = bin(ctx, BinOp::And, Operand::Value(word), mask, Type::I64);
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(rest, obj_val, 8));
        let set = bin(ctx, BinOp::LShr, rest, Operand::ConstI64(32), Type::I64);
        let empty = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, set, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let drop_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(empty),
                then_blk: drop_blk,
                else_blk: store_blk,
            },
        );
        ctx.cur_block = drop_blk;
        let hdr = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I32, obj_val, 4),
            Type::I32,
            None,
        );
        let lowered = bin(
            ctx,
            BinOp::And,
            Operand::Value(hdr),
            Operand::ConstI32(!flag),
            Type::I32,
        );
        ctx.f
            .append_void(ctx.cur_block, InstKind::Store(lowered, obj_val, 4));
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(store_blk));
    ctx.cur_block = exotic_blk;
}
