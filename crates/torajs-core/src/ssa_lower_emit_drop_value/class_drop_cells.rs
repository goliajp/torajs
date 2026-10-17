//! The per-cell steps a `__drop_<C>` release fn is built from: the
//! nullish and tag tests, the count decrement, the WeakRef and props
//! seams and the buffered-root deferral. Split from `class_drop.rs`
//! (file-size limit).

use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Operand, Terminator, Type,
};
use crate::ssa_lower::{OBJ_CLASS_TAG_OFF, OBJ_PROPS_OFF};
use crate::ssa_lower_emit_drop_value::class_drop::Cx;

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

/// `p == NULL`, the absent child a nullable `Obj` slot holds most.
pub(crate) fn emit_is_null(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
    let is_null = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Eq, p, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    Operand::Value(is_null)
}

/// `p == @undefined_cell`, the other value a nullable `Obj` slot
/// holds that owns nothing (`nullish_skip_cond`).
pub(crate) fn emit_is_undef(f: &mut ssa::Function, blk: BlockId, p: Operand) -> Operand {
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
    Operand::Value(is_undef)
}

/// Leave `blk` for `nullish` when `p` is null or undefined, one test
/// per branch; answers the block the owning path continues in.
pub(crate) fn branch_nullish(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    nullish: BlockId,
) -> BlockId {
    let undef_blk = f.add_block();
    let owned = f.add_block();
    let is_null = emit_is_null(f, blk, p);
    branch(f, blk, is_null, nullish, undef_blk);
    let is_undef = emit_is_undef(f, undef_blk, p);
    branch(f, undef_blk, is_undef, nullish, owned);
    owned
}

/// The low 32 bits of the +8 slot against each tag of the sid (the
/// install cursor shares the word's high half): a 32-bit load reads
/// exactly the half `load_obj_class_tag` masks out.
pub(crate) fn emit_tag_in_set(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    tags: &[u32],
) -> Operand {
    let tag = f.append_inst(
        blk,
        InstKind::Load(Type::I32, p, OBJ_CLASS_TAG_OFF),
        Type::I32,
        None,
    );
    let mut acc: Option<Operand> = None;
    for &t in tags {
        let hit = f.append_inst(
            blk,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI32(t as i32)),
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
/// (the inline `torajs_rc::__torajs_weakref_active` read, or the
/// worklist's one read). Answers the block the caller continues in.
pub(crate) fn emit_weakref_notify(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    p: Operand,
) -> BlockId {
    let active = match cx.worklist.as_ref().and_then(|wl| wl.active) {
        Some(active) => active,
        None => {
            let ptr = f.append_inst(
                blk,
                InstKind::GlobalRef(ssa::WEAKREF_ACTIVE_SYM.to_string()),
                Type::Ptr,
                None,
            );
            let load = InstKind::Load(Type::I64, Operand::Value(ptr), 0);
            Operand::Value(f.append_inst(blk, load, Type::I64, None))
        }
    };
    let observed = f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, active, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    emit_cold_call(f, cx, blk, observed, cx.intr.weakref_target_dying, p)
}

/// `p.props != NULL` — the expando bag's two link seams (release,
/// cycle-root buffering) run only for a cell that has one.
pub(crate) fn emit_has_props(f: &mut ssa::Function, blk: BlockId, p: Operand) -> ssa::ValueId {
    let props = f.append_inst(
        blk,
        InstKind::Load(Type::Ptr, p, OBJ_PROPS_OFF),
        Type::Ptr,
        None,
    );
    f.append_inst(
        blk,
        InstKind::ICmp(IPred::Ne, Operand::Value(props), Operand::ConstPtrNull),
        Type::Bool,
        None,
    )
}

/// `if (cond) fid(arg)` in place — answers the join block.
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

/// `if (cond) fid(arg)` with the call laid out past the fn's hot
/// path, so the common case falls straight through — answers the
/// join block.
pub(crate) fn emit_cold_call(
    f: &mut ssa::Function,
    cx: &Cx<'_>,
    blk: BlockId,
    cond: ssa::ValueId,
    fid: FuncId,
    arg: Operand,
) -> BlockId {
    let join_blk = f.add_block();
    cx.defer(move |f| {
        let call_blk = f.add_block();
        branch(f, blk, Operand::Value(cond), call_blk, join_blk);
        f.append_void(call_blk, InstKind::Call(fid, vec![arg]));
        f.set_term(call_blk, Terminator::Br(join_blk));
    });
    join_blk
}

/// `FLAG_BUFFERED` of the header's flags word: a cell the cycle
/// collector holds as a candidate root. Answers the test and the
/// word `emit_colour_black` rewrites.
pub(crate) fn emit_is_buffered(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
) -> (Operand, ssa::ValueId) {
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
    (Operand::Value(buffered), word)
}

/// A buffered cell stays allocated, coloured black with rc 0, for
/// the next collect's root walk to free.
pub(crate) fn emit_colour_black(
    f: &mut ssa::Function,
    blk: BlockId,
    p: Operand,
    word: ssa::ValueId,
) {
    let black = f.append_inst(
        blk,
        InstKind::BinOp(
            SsaBinOp::And,
            Operand::Value(word),
            Operand::ConstI32(!((torajs_rc::COLOR_MASK as u32) << 16) as i32),
        ),
        Type::I32,
        None,
    );
    f.append_void(blk, InstKind::Store(Operand::Value(black), p, 4));
}
