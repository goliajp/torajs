//! Rule 7, the hole test: a typed read of a pointer element whose slot
//! is NULL answers `undefined` instead.

use crate::ssa::{BlockId, Function, IPred, InstKind, Operand, Terminator};
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;

use super::defs::{Defs, value_def};

/// The arm `icmp eq|ne (load …), null` takes on a NULL slot, when that
/// arm only spells `undefined`: a `global_ref` of the generic or the
/// string undefined cell, moved into the read's result (`copy`, or a
/// `store` before mem2reg), then `br` to the join. A slot is NULL only
/// where the program grew the array past a gap and then reads the gap,
/// so reading the element is the arm that runs.
pub(super) fn hole_arm(
    func: &Function,
    blocks: &[usize],
    cond: &InstKind,
    defs: &Defs,
    then_blk: BlockId,
    else_blk: BlockId,
) -> Option<BlockId> {
    let InstKind::ICmp(pred @ (IPred::Eq | IPred::Ne), x, Operand::ConstPtrNull) = cond else {
        return None;
    };
    let loaded = matches!(
        value_def(x, defs),
        Some(InstKind::Load(..) | InstKind::LoadDyn(..) | InstKind::LoadDynScaled8(..))
    );
    let null_arm = if *pred == IPred::Eq {
        then_blk
    } else {
        else_blk
    };
    (loaded && spells_only_undefined(func, blocks, null_arm)).then_some(null_arm)
}

fn spells_only_undefined(func: &Function, blocks: &[usize], b: BlockId) -> bool {
    let block = &func.blocks[blocks[b.0 as usize]];
    let [cell, moves @ ..] = block.insts.as_slice() else {
        return false;
    };
    let (Some(cell_v), InstKind::GlobalRef(sym)) = (cell.result, &cell.kind) else {
        return false;
    };
    let is_cell = |op: &Operand| *op == Operand::Value(cell_v);
    (sym == UNDEF_CELL_SYM || sym == STR_UNDEF_CELL_SYM)
        && matches!(block.term, Terminator::Br(_))
        && moves.iter().all(|i| match &i.kind {
            InstKind::Copy(_, src) => is_cell(src),
            InstKind::Store(val, _, 0) => is_cell(val),
            _ => false,
        })
}
