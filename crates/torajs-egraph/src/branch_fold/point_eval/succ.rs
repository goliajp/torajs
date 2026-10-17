//! A terminator's successor blocks.

use torajs_core::ssa::{BlockId, Terminator};

pub(crate) fn successors(term: &Terminator) -> impl Iterator<Item = BlockId> + '_ {
    let (a, b) = match term {
        Terminator::Br(t) => (Some(*t), None),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => (Some(*then_blk), Some(*else_blk)),
        Terminator::Ret(_) | Terminator::Unreachable => (None, None),
    };
    a.into_iter().chain(b)
}
