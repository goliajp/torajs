//! Merge bridges — demotion containment for accumulator cells read
//! after the loop (`console.log(sum)` / `return sum`). An escaped
//! in-set cell gets a fresh f64 bridge cell: every external use reads
//! the bridge, and every region exit edge (fast and slow) writes it —
//! `sitofp` on the fast side (exact, the guard invariant keeps every
//! in-set value at ≤ 2^53 magnitude), a plain copy of the cloned f64
//! value on the slow side. External uses then observe the merged
//! trajectory no matter which version ran, and an escaping use is no
//! longer an eviction reason.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use crate::slot_forward::rewrite_operands;

use super::apply::RegionClone;
use super::cfg::{remap_term_operand, retarget_edge, successors};
use super::plan::RegionPlan;
use super::push_value;

/// The bridge cells of one region: (escaped value, fresh f64 cell),
/// sorted by the escaped value id.
pub(crate) struct MergeBridge {
    cells: Vec<(ValueId, ValueId)>,
}

impl MergeBridge {
    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }
}

/// Mint a bridge cell per escaped value and re-point every external
/// use at it. Runs before cloning (only original blocks exist, so the
/// walk skips exactly the region) and before mutation (the rewritten
/// uses must not take per-use `sitofp` bridges).
pub(crate) fn rewrite_escapes(func: &mut Function, plan: &RegionPlan) -> MergeBridge {
    let cells: Vec<(ValueId, ValueId)> = plan
        .escapes
        .iter()
        .map(|v| (*v, push_value(&mut func.values, Type::F64)))
        .collect();
    if cells.is_empty() {
        return MergeBridge { cells };
    }
    let region: HashSet<BlockId> = plan.region.iter().copied().collect();
    let op_map: HashMap<ValueId, Operand> = cells
        .iter()
        .map(|(v, w)| (*v, Operand::Value(*w)))
        .collect();
    for bi in 0..func.blocks.len() {
        if region.contains(&BlockId(bi as u32)) {
            continue;
        }
        let block = &mut func.blocks[bi];
        for inst in &mut block.insts {
            rewrite_operands(&mut inst.kind, &op_map);
        }
        let term = std::mem::replace(&mut block.term, Terminator::Unreachable);
        block.term = remap_term_operand(term, &op_map);
    }
    MergeBridge { cells }
}

/// Install the exit writebacks after mutation. Runs before
/// `install_guards`: region terminators are not split yet (exit edges
/// still sit on the planned blocks), and the slow-side writeback
/// blocks get picked up by its liveness walk, keeping the cloned
/// values alive into the side-exit state.
pub(crate) fn install_exits(
    func: &mut Function,
    plan: &RegionPlan,
    clone: &RegionClone,
    bridge: &MergeBridge,
) {
    if bridge.cells.is_empty() {
        return;
    }
    let region: HashSet<BlockId> = plan.region.iter().copied().collect();
    let slow: HashSet<BlockId> = clone.block_map.values().copied().collect();

    // fast exits: the demoted i64 cell bridges through sitofp
    for &b in &plan.region {
        for s in exit_targets(func, b, |s| !region.contains(&s)) {
            let insts = bridge
                .cells
                .iter()
                .map(|(v, w)| Inst {
                    result: Some(*w),
                    kind: InstKind::SiToFp(Operand::Value(*v)),
                    origin: None,
                })
                .collect();
            let m = append_block(func, insts, s);
            retarget_edge(&mut func.blocks[b.0 as usize].term, s, m);
        }
    }
    // slow exits: the cloned value is already f64. Walked through
    // `plan.region` rather than `block_map.values()` — the map's keys
    // ARE the region, and iterating the map would hand the writeback
    // blocks their ids in hash order (rotation 536: `array-sum-1m`
    // built two distinct artifacts once it became versionable, with
    // nothing but two block numbers swapped).
    for &b in &plan.region {
        let cb = clone.block_map[&b];
        for s in exit_targets(func, cb, |s| !slow.contains(&s) && !region.contains(&s)) {
            let insts = bridge
                .cells
                .iter()
                .map(|(v, w)| Inst {
                    result: Some(*w),
                    kind: InstKind::Copy(Type::F64, Operand::Value(clone.value_map[v])),
                    origin: None,
                })
                .collect();
            let m = append_block(func, insts, s);
            retarget_edge(&mut func.blocks[cb.0 as usize].term, s, m);
        }
    }
}

/// Deduplicated exit targets of one block (a CondBr with both arms on
/// the same external target retargets once, to one writeback block).
fn exit_targets(func: &Function, b: BlockId, is_exit: impl Fn(BlockId) -> bool) -> Vec<BlockId> {
    let mut out: Vec<BlockId> = Vec::new();
    for s in successors(&func.blocks[b.0 as usize].term) {
        if is_exit(s) && !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn append_block(func: &mut Function, insts: Vec<Inst>, target: BlockId) -> BlockId {
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts,
        term: Terminator::Br(target),
    });
    id
}
