//! Rule 4, the length guard: an unsigned compare against the array
//! length word, or against the register cell `len_promote` keeps it in.

use crate::ssa::{BlockId, Function, IPred, InstKind, Operand, ValueId};

use super::defs::{Defs, value_def};

/// Byte offset of an array's length word in its heap header.
const ARR_LEN_OFFSET: u64 = 8;

fn is_len_load(kind: Option<&InstKind>) -> bool {
    matches!(kind, Some(InstKind::Load(_, _, off)) if *off == ARR_LEN_OFFSET)
}

/// The multi-def values one of whose defs copies a length word: the
/// cell `len_promote` keeps a loop's array length in (its preheader
/// def reads the word; the rest are the loop's length stores and the
/// re-reads after a kernel call).
pub(super) fn length_cells(func: &Function, defs: &Defs) -> Vec<bool> {
    let mut out = vec![false; defs.0.len()];
    for b in &func.blocks {
        for i in &b.insts {
            if let (Some(r), InstKind::Copy(_, src)) = (i.result, &i.kind)
                && value_def(&Operand::Value(r), defs).is_none()
                && is_len_load(value_def(src, defs))
            {
                out[r.0 as usize] = true;
            }
        }
    }
    out
}

/// The arm an unsigned compare against the array length word takes
/// when the index is out of bounds.
pub(super) fn out_of_bounds_arm(
    cond: &InstKind,
    defs: &Defs,
    in_cell: &impl Fn(ValueId) -> bool,
    then_blk: BlockId,
    else_blk: BlockId,
) -> Option<BlockId> {
    let InstKind::ICmp(pred @ (IPred::Ult | IPred::Uge), _, len) = cond else {
        return None;
    };
    let in_cell = |op: &Operand| matches!(op, Operand::Value(v) if in_cell(*v));
    let is_len = is_len_load(value_def(len, defs))
        || in_cell(len)
        || matches!(value_def(len, defs), Some(InstKind::Copy(_, c)) if in_cell(c));
    is_len.then_some(if *pred == IPred::Ult {
        else_blk
    } else {
        then_blk
    })
}
