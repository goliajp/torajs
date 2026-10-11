//! The stores a literal that leaves members out adds to the cell it
//! builds: the absent set, or the deletion records past the set's reach.

use super::{FLAG_SHIFT, SET_SHIFT, SET_SLOTS, SET_WORD_OFF, bin};
use crate::ast::PropKey;
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Whether every absent slot the literal records fits the set; a slot
/// past it puts the whole literal on the deletion records.
pub(crate) fn fits_set(slots: impl IntoIterator<Item = usize>) -> bool {
    slots.into_iter().all(|i| i < SET_SLOTS)
}

/// Raise the absent set on the finished cell `obj`: `fixed` names the
/// slots the literal leaves out, `cond` the slots a spread source may
/// lack (I64 1 / 0 each, nonzero when every source that contributed it
/// lacked it). `hdr` is the header word the cell was initialized with;
/// the bit goes up only when the set is not empty. The set joins the
/// class tag in the `+8` word, which keeps every access to the word the
/// one width a scalar replacement of the cell reads.
pub(crate) fn raise_set(
    ctx: &mut LowerCtx<'_>,
    obj: ValueId,
    hdr: i32,
    fixed: &[usize],
    cond: &[(usize, Operand)],
) {
    let fixed_bits = fixed.iter().fold(0u64, |acc, i| acc | 1 << i) << SET_SHIFT;
    if cond.is_empty() && fixed_bits == 0 {
        return;
    }
    let mut set = Operand::ConstI64(fixed_bits as i64);
    for (i, lacks) in cond {
        let at = Operand::ConstI64((SET_SHIFT + *i as u32) as i64);
        let bit = bin(ctx, BinOp::Shl, *lacks, at);
        set = bin(ctx, BinOp::Or, set, bit);
    }
    let cont = match cond.is_empty() {
        true => None,
        false => {
            let any = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, set, Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let (mark, cont) = (ctx.f.add_block(), ctx.f.add_block());
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(any),
                    then_blk: mark,
                    else_blk: cont,
                },
            );
            ctx.cur_block = mark;
            Some(cont)
        }
    };
    let flag = (u32::from(torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD) << FLAG_SHIFT) as i32;
    let cell = Operand::Value(obj);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI32(hdr | flag), cell, 4),
    );
    let word = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, cell, SET_WORD_OFF),
        Type::I64,
        None,
    );
    let word = bin(ctx, BinOp::Or, Operand::Value(word), set);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(word, cell, SET_WORD_OFF));
    if let Some(cont) = cont {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cont));
        ctx.cur_block = cont;
    }
}

/// Record each absent field deleted on the finished cell — the record a
/// `delete` of a declared field leaves, written straight (a fresh
/// literal cell has nothing the delete kernel's gates could refuse) —
/// for a literal whose absent slots the set cannot name. `cond` as in
/// [`raise_set`].
pub(crate) fn record_deleted(
    ctx: &mut LowerCtx<'_>,
    obj: ValueId,
    fixed: &[PropKey],
    cond: &[(PropKey, Operand)],
) {
    for name in fixed {
        delete_record(ctx, obj, name);
    }
    for (name, lacks) in cond {
        let hit = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, *lacks, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let (del_blk, cont) = (ctx.f.add_block(), ctx.f.add_block());
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(hit),
                then_blk: del_blk,
                else_blk: cont,
            },
        );
        ctx.cur_block = del_blk;
        delete_record(ctx, obj, name);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(cont));
        ctx.cur_block = cont;
    }
}

fn delete_record(ctx: &mut LowerCtx<'_>, obj: ValueId, name: &PropKey) {
    let key = ctx.intern_string_literal(name);
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.struct_expando.obj_field_delete,
            vec![Operand::Value(obj), Operand::Value(key)],
        ),
        Type::I64,
        None,
    );
}
