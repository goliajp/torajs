//! The write-back a cluster entry makes of its scalars after it calls
//! its body, which a probe of the throw flag may look past.

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{Function, InstKind, Operand, PinWord, ValueId};

/// A cluster entry's write-back of its scalars in `func`, which neither
/// calls nor reads x21: the `pinned` read of each scalar, with the
/// offset of the receiver's field the entry loaded the scalar from.
pub(super) struct WriteBack {
    this: Option<ValueId>,
    reads: HashMap<ValueId, u64>,
}

impl WriteBack {
    pub(super) fn of(func: &Function) -> Self {
        let this = func.params.first().copied();
        let insts = || func.blocks.iter().flat_map(|b| &b.insts);
        let loaded: HashMap<ValueId, u64> = (insts())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Load(_, Operand::Value(b), off), Some(r)) if Some(*b) == this => {
                    Some((r, *off))
                }
                _ => None,
            })
            .collect();
        let offs: HashMap<u8, u64> = (insts())
            .filter_map(|i| match &i.kind {
                InstKind::PinSet(PinWord::Scalar(k), Operand::Value(l)) => {
                    loaded.get(l).map(|o| (*k, *o))
                }
                _ => None,
            })
            .collect();
        let reads = (insts())
            .filter_map(|i| match (&i.kind, i.result) {
                (InstKind::Pinned(PinWord::Scalar(k)), Some(r)) => offs.get(k).map(|o| (r, *o)),
                _ => None,
            })
            .collect();
        WriteBack { this, reads }
    }

    /// Whether `kind` is part of the write-back: a `pinned` read of a
    /// scalar, or its store into the scalar's field of the receiver.
    pub(super) fn contains(&self, kind: &InstKind) -> bool {
        match kind {
            InstKind::Pinned(PinWord::Scalar(_)) => true,
            InstKind::Store(Operand::Value(v), Operand::Value(b), off) => {
                Some(*b) == self.this && self.reads.get(v) == Some(off)
            }
            _ => false,
        }
    }
}
