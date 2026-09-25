//! Cell-model soundness for the slot passes (`slot_forward` /
//! `mem2reg` / `phi_promote`). They treat each `(slot, offset)` pair
//! as an independent cell, but the aarch64 backend lowers every
//! `Load` / `Store` as an 8-byte access (`compile/mem.rs`), so two
//! offsets closer than 8 bytes share memory: an object header's
//! refcount store at +0 and type-tag store at +4 each write the
//! other's bytes. A cell that overlaps another accessed offset of the
//! same slot must keep its memory — forwarding it would read the
//! value the cell model believes in, not the bytes the machine holds.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, ValueId};

/// Width of every non-dynamic slot access in the backend.
const ACCESS_BYTES: u64 = 8;

/// The `(slot, offset)` cells of `slots` whose 8-byte window overlaps
/// another accessed offset of the same slot.
pub(crate) fn overlapping_cells(
    func: &Function,
    slots: &HashSet<ValueId>,
) -> HashSet<(ValueId, u64)> {
    let mut offs: HashMap<ValueId, Vec<u64>> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let InstKind::Load(_, Operand::Value(s), off)
            | InstKind::Store(_, Operand::Value(s), off) = &inst.kind
                && slots.contains(s)
            {
                offs.entry(*s).or_default().push(*off);
            }
        }
    }
    let mut out = HashSet::new();
    for (s, mut v) in offs {
        v.sort_unstable();
        v.dedup();
        for w in v.windows(2) {
            if w[1] - w[0] < ACCESS_BYTES {
                out.insert((s, w[0]));
                out.insert((s, w[1]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Terminator, Type, ValueInfo};

    #[test]
    fn header_offsets_overlap_fields_do_not() {
        let s = ValueId(0);
        let inst = |result, kind| Inst {
            result,
            kind,
            origin: None,
        };
        let st = |off| {
            inst(
                None,
                InstKind::Store(Operand::ConstI64(1), Operand::Value(s), off),
            )
        };
        let f = Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    inst(Some(s), InstKind::AllocaBytes(56)),
                    st(0),
                    st(4),
                    st(16),
                    st(48),
                ],
                term: Terminator::Ret(None),
            }],
            values: vec![ValueInfo {
                ty: Type::Ptr,
                name: None,
            }],
            current_origin: None,
        };
        let slots: HashSet<ValueId> = [s].into_iter().collect();
        let want: HashSet<(ValueId, u64)> = [(s, 0), (s, 4)].into_iter().collect();
        assert_eq!(overlapping_cells(&f, &slots), want);
    }
}
