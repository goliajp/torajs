//! The per-cell steps a `__drop_<C>` release fn is built from: the
//! nullish and tag tests, the count decrement, the WeakRef and props
//! seams and the buffered-root deferral. Split from `class_drop.rs`
//! (file-size limit).

use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type,
};
use crate::ssa_lower::{OBJ_CLASS_TAG_OFF, OBJ_PROPS_OFF};

pub(crate) fn branch(
    f: &mut ssa::Function,
    blk: BlockId,
    cond: Operand,
    then_blk: BlockId,
    else_blk: BlockId,
) {
    f.set_term(
        blk,
        Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        },
    );
}

/// `p == NULL | p == @undefined_cell` — the two values a nullable
/// `Obj` slot holds that own nothing (`nullish_skip_cond`).
pub(crate) fn emit_nullish(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
    let is_null = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, p, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let undef = f.append_inst(
        blk,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let is_undef = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, p, Operand::Value(undef)),
        Type::Bool,
        None,
    );
    let nullish = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(is_null),
            Operand::Value(is_undef),
        ),
        Type::Bool,
        None,
    );
    Operand::Value(nullish)
}

/// The low 32 bits of the +8 slot against each tag of the sid
/// (`load_obj_class_tag`'s mask: the install cursor shares the word).
pub(crate) fn emit_tag_in_set(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    tags: &[u32],
) -> Operand {
    let slot = f.append_inst(
        blk,
        InstKind::Load(Type::I64, p, OBJ_CLASS_TAG_OFF),
        Type::I64,
        None,
    );
    let tag = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(slot),
            Operand::ConstI64(0xFFFF_FFFF),
        ),
        Type::I64,
        None,
    );
    let mut acc: Option<Operand> = None;
    for &t in tags {
        let hit = f.append_inst(
            blk,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(t as i64)),
            Type::Bool,
            None,
        );
        acc = Some(match acc {
            None => Operand::Value(hit),
            Some(prev) => Operand::Value(f.append_inst(
                blk,
                InstKind::BinOp(SsaBinOp::Or, prev, Operand::Value(hit)),
                Type::Bool,
                None,
            )),
        });
    }
    acc.expect("a sid's tag set is never empty")
}

/// `--rc == 0`, the header write `emit_rc_dec_inline` makes.
pub(crate) fn emit_rc_dec(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
    let rc = f.append_inst(blk, InstKind::Load(Type::I32, p, 0), Type::I32, None);
    let rc_new = f.append_inst(
        blk,
        InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc), Operand::ConstI32(1)),
        Type::I32,
        None,
    );
    f.append_void(blk, InstKind::Store(Operand::Value(rc_new), p, 0));
    let is_zero = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, Operand::Value(rc_new), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    Operand::Value(is_zero)
}

/// Notify the WeakRef registry only while something observes
/// (the inline `torajs_rc::__torajs_weakref_active` read). Answers
/// the block the caller continues in.
pub(crate) fn emit_weakref_notify(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    dying: FuncId,
) -> BlockId {
    let active_ptr = f.append_inst(
        blk,
        InstKind::GlobalRef(ssa::WEAKREF_ACTIVE_SYM.to_string()),
        Type::Ptr,
        None,
    );
    let active = f.append_inst(
        blk,
        InstKind::Load(Type::I64, Operand::Value(active_ptr), 0),
        Type::I64,
        None,
    );
    let observed = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    emit_cond_call(f, blk, observed, dying, p)
}

/// `if (p.props != NULL) seam(p)` — the expando bag's two link seams
/// (release, cycle-root buffering) run only for a cell that has one.
pub(crate) fn emit_if_props(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    seam: FuncId,
) -> BlockId {
    let props = f.append_inst(
        blk,
        InstKind::Load(Type::Ptr, p, OBJ_PROPS_OFF),
        Type::Ptr,
        None,
    );
    let nonnull = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(props), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    emit_cond_call(f, blk, nonnull, seam, p)
}

/// `if (cond) fid(arg)` — answers the join block.
pub(crate) fn emit_cond_call(
    f: &mut ssa::Function,
    blk: BlockId,
    cond: ssa::ValueId,
    fid: FuncId,
    arg: Operand,
) -> BlockId {
    let call_blk = f.add_block();
    let join_blk = f.add_block();
    branch(f, blk, Operand::Value(cond), call_blk, join_blk);
    f.append_void(call_blk, InstKind::Call(fid, vec![arg]));
    f.set_term(call_blk, Terminator::Br(join_blk));
    join_blk
}

/// A cell the cycle collector holds as a candidate root stays
/// allocated, coloured black with rc 0, for the next collect's root
/// walk to free; that leg goes to `after`. Answers the free leg.
pub(crate) fn emit_defer_if_buffered(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    after: BlockId,
) -> BlockId {
    let word = f.append_inst(blk, InstKind::Load(Type::I32, p, 4), Type::I32, None);
    let bit = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(((torajs_rc::FLAG_BUFFERED as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    let buffered = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(bit), Operand::ConstI32(0)),
        Type::Bool,
        None,
    );
    let defer_blk = f.add_block();
    let free_blk = f.add_block();
    branch(f, blk, Operand::Value(buffered), defer_blk, free_blk);
    let black = f.append_inst(
        defer_blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(!((torajs_rc::COLOR_MASK as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    f.append_void(defer_blk, InstKind::Store(Operand::Value(black), p, 4));
    f.set_term(defer_blk, Terminator::Br(after));
    free_blk
}
