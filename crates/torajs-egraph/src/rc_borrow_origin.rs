//! Which origins `rc_borrow` may borrow from: the inline element read
//! of a dense `any[]`, whose slot owns a share of the box it answers.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, Function, HEADER_WORD_ARR_ANY, IPred, InstKind, Operand, Terminator, Type, ValueId,
};

use torajs_core::ssa_lower::ARR_DATA_PTR_OFF;

use crate::rc_borrow::Site;

/// Is the origin at `site` an element read of a dense `any[]`: a
/// `load_dyn any` through the array's buffer pointer, in a block
/// entered only from the header test for `FLAG_ARR_ANY` on that
/// array?
pub(crate) fn is_array_elem_read(
    func: &Function,
    site: Site,
    defs: &HashMap<ValueId, Vec<Site>>,
    preds: &[Vec<usize>],
) -> bool {
    let (b, i) = site;
    let kind_of = |v: ValueId| match defs.get(&v).map(Vec::as_slice) {
        Some(&[(db, di)]) => Some(&func.blocks[db].insts[di].kind),
        _ => None,
    };
    let const_i64 = |op: &Operand| match op {
        Operand::ConstI64(k) => Some(*k),
        Operand::Value(v) => match kind_of(*v) {
            Some(InstKind::Copy(Type::I64, Operand::ConstI64(k))) => Some(*k),
            _ => None,
        },
        _ => None,
    };
    let (InstKind::LoadDyn(Type::Any, Operand::Value(data), _)
    | InstKind::LoadDynScaled8(Type::Any, Operand::Value(data), _)) = &func.blocks[b].insts[i].kind
    else {
        return false;
    };
    let Some(InstKind::Load(Type::Ptr, Operand::Value(arr), ARR_DATA_PTR_OFF)) = kind_of(*data)
    else {
        return false;
    };
    let [p] = preds[b].as_slice() else {
        return false;
    };
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        ..
    } = &func.blocks[*p].term
    else {
        return false;
    };
    if then_blk.0 as usize != b {
        return false;
    }
    let Some(InstKind::ICmp(IPred::Eq, Operand::Value(flags), word)) = kind_of(*c) else {
        return false;
    };
    if const_i64(word) != Some(HEADER_WORD_ARR_ANY) {
        return false;
    }
    let Some(InstKind::BinOp(BinOp::And, Operand::Value(w), mask)) = kind_of(*flags) else {
        return false;
    };
    if const_i64(mask).is_none_or(|m| m & HEADER_WORD_ARR_ANY == 0) {
        return false;
    }
    matches!(kind_of(*w), Some(InstKind::Load(Type::I64, Operand::Value(a), 0)) if a == arr)
}
