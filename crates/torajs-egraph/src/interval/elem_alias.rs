//! Which instructions make a second name for a tracked array or for its
//! data pointer. Split from `elem.rs` at the file-size line.

use std::collections::HashMap;

use torajs_core::ssa::{BinOp, InstKind, Operand, ValueId};

use super::elem::{name_of, val};
use super::elem_kernels::*;

pub(super) enum AliasKind {
    /// Same thing as the source operand.
    Same,
    /// The data pointer of the source array.
    DataPtr,
    /// A data pointer moved forward within its own buffer (the fill
    /// loop's slot[0] address). Only a data pointer qualifies, and only
    /// by a non-negative constant or a whole number of slots (`x << 3`):
    /// a write through the result must still land among the slots.
    Advanced,
}

pub(super) fn alias_source(
    kind: &InstKind,
    names: &[String],
    defs: &HashMap<ValueId, Vec<InstKind>>,
) -> Option<(ValueId, AliasKind)> {
    let whole_slots = |off: &Operand| match off {
        Operand::ConstI64(k) => *k >= 0,
        Operand::Value(o) => matches!(
            defs.get(o).map(Vec::as_slice),
            Some([InstKind::BinOp(BinOp::Shl, _, Operand::ConstI64(3))])
        ),
        _ => false,
    };
    match kind {
        InstKind::Copy(_, op) | InstKind::Identity(op) => val(op).map(|v| (v, AliasKind::Same)),
        InstKind::Load(_, op, ARR_DATA_PTR_OFF) => val(op).map(|v| (v, AliasKind::DataPtr)),
        InstKind::BinOp(BinOp::Add, op, off) if whole_slots(off) => {
            val(op).map(|v| (v, AliasKind::Advanced))
        }
        InstKind::Call(f, args) => {
            let n = name_of(names, *f);
            if IDENTITY_CALLS.contains(&n) || IDENTITY_PUSH_CALLS.contains(&n) {
                args.first().and_then(val).map(|v| (v, AliasKind::Same))
            } else {
                None
            }
        }
        _ => None,
    }
}
