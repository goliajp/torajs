//! The per-block walk of a pending retain in [`super::inc_sink`]: how
//! far a `rc_inc_any` can move inside one block, and where it lands
//! at the block's exit.

use torajs_core::ssa::{Function, InstKind, Operand, Terminator, Type, ValueId};

use crate::anynum::expand::is_number_node;
use crate::optimize::classify_pure;

/// Where the walk of one block ends.
pub(super) enum Walk {
    /// Before this instruction, retaining this value.
    Place(usize, ValueId),
    /// Past the terminator, retaining this value.
    Exit(ValueId),
}

/// Walk `insts[start..]` of block `b` with a pending retain of `v`.
pub(super) fn walk(func: &Function, b: usize, start: usize, mut v: ValueId) -> Walk {
    let insts = &func.blocks[b].insts;
    for (k, inst) in insts.iter().enumerate().skip(start) {
        if inst.result == Some(v) {
            return Walk::Place(k, v);
        }
        if let (InstKind::Copy(Type::Any, Operand::Value(s)), Some(r)) = (&inst.kind, inst.result)
            && *s == v
        {
            v = r;
            continue;
        }
        if !transparent(&inst.kind) {
            return Walk::Place(k, v);
        }
    }
    Walk::Exit(v)
}

/// Can a pending retain move past `kind`: nothing that lowers a
/// count, frees, or writes a heap slot.
///
/// A read of a header's count is not: an inline release is `load i32
/// [p+0]; sub 1; store [p+0]`, and a retain of the same object placed
/// between the load and the store is overwritten by the store, one
/// reference lost. The walk stops at the load, ahead of the whole
/// release, whatever the blocks around it look like.
fn transparent(kind: &InstKind) -> bool {
    if matches!(kind, InstKind::Load(Type::I32, _, 0)) {
        return false;
    }
    classify_pure(kind)
        || is_number_node(kind)
        || matches!(
            kind,
            InstKind::Copy(..)
                | InstKind::Load(..)
                | InstKind::LoadDyn(..)
                | InstKind::LoadDynScaled8(..)
                | InstKind::LoadU8Dyn(..)
                | InstKind::LoadU16Dyn(..)
                | InstKind::GlobalRef(_)
                | InstKind::StaticStrRef(_)
                | InstKind::StringRef(_)
                | InstKind::PtrToInt(_)
                | InstKind::IntToPtr(_)
                | InstKind::BitCastF64ToI64(_)
                | InstKind::BitCastI64ToF64(_)
                | InstKind::TruncI64ToBool(_)
                | InstKind::BoxAny(..)
                | InstKind::Alloca(_)
                | InstKind::AllocaBytes(_)
        )
}

/// Where a retain placed at the exit of block `b` (walked from
/// `start`) goes: ahead of the compare its conditional branch reads
/// when that compare ends the block, so the compare stays next to the
/// branch. Behind it, the inline retain's own tests would clobber the
/// flags and the compare would have to be kept in a register.
pub(super) fn exit_at(func: &Function, b: usize, start: usize) -> usize {
    let block = &func.blocks[b];
    let end = block.insts.len();
    match (&block.term, block.insts.last()) {
        (
            Terminator::CondBr {
                cond: Operand::Value(c),
                ..
            },
            Some(last),
        ) if last.result == Some(*c) && end > start => end - 1,
        _ => end,
    }
}
