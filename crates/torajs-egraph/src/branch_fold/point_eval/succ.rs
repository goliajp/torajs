//! A terminator's successor blocks.

use torajs_core::ssa::{BlockId, Terminator};

pub(crate) fn successors(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}
