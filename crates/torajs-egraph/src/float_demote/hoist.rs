//! Guard widening for a growth site on one arm of a diamond. A pre-op
//! check whose block hangs off a two-way branch moves to the entry of
//! the branching block, and its side exit re-enters the slow clone of
//! that block instead of the arm's split point. The fast diamond then
//! holds no branch but the parity test, so select formation can turn
//! it into a conditional select; the widened check is a branch that
//! never fires while the value stays in its window.
//!
//! Widening `arm ∧ v > bound` to `v > bound` is sound because the
//! slow clone recomputes from the branching block's entry state: it
//! only costs a slower iteration when the other arm would have stayed
//! exact. The conditions keep that re-execution invisible (LLVM's
//! GuardWidening makes the same trade):
//!   - pre-op checks only (a post check reads the op's result);
//!   - the branching block P is in the loop body but not its header
//!     (every predecessor is in the body), and is the site block's
//!     sole predecessor, so P dominates the site;
//!   - everything from P's entry up to the site is pure, defines no
//!     multi-def cell (shared cells are not written back, so a re-run
//!     would apply their update twice) and does not redefine the
//!     checked operand.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, Function, Inst, Operand, Terminator, ValueId};

use super::cfg::pred_map;
use super::guard::GuardSite;
use super::plan::RegionPlan;
use crate::optimize::is_pure;

/// Site key → the block its checks move to. Runs after mutation:
/// `fast_pos` gives each site's index in its mutated block. At most
/// one site lands in a block, and never in a block that holds a site.
pub(super) fn widen_targets(
    func: &Function,
    plan: &RegionPlan,
    fast_pos: &HashMap<(BlockId, usize), usize>,
    cells: &HashSet<ValueId>,
) -> HashMap<(BlockId, usize), BlockId> {
    let preds = pred_map(func);
    let body: HashSet<BlockId> = plan.body.iter().copied().collect();
    let site_blocks: HashSet<BlockId> = plan.growth.iter().map(|s| s.block).collect();
    let mut taken: HashSet<BlockId> = HashSet::default();
    let mut out = HashMap::default();
    for s in &plan.growth {
        let Some(p) = target(func, &preds, &body, s, fast_pos, cells) else {
            continue;
        };
        if site_blocks.contains(&p) || !taken.insert(p) {
            continue;
        }
        out.insert((s.block, s.inst_idx), p);
    }
    out
}

fn target(
    func: &Function,
    preds: &[Vec<BlockId>],
    body: &HashSet<BlockId>,
    s: &GuardSite,
    fast_pos: &HashMap<(BlockId, usize), usize>,
    cells: &HashSet<ValueId>,
) -> Option<BlockId> {
    if s.post {
        return None;
    }
    let [p] = preds[s.block.0 as usize][..] else {
        return None;
    };
    if !body.contains(&p) || !preds[p.0 as usize].iter().all(|q| body.contains(q)) {
        return None;
    }
    let Terminator::CondBr {
        then_blk, else_blk, ..
    } = func.blocks[p.0 as usize].term
    else {
        return None;
    };
    if then_blk == else_blk {
        return None;
    }
    let operands: Vec<_> = s
        .checks
        .iter()
        .filter_map(|c| match c.operand {
            Operand::Value(v) => Some(v),
            _ => None,
        })
        .collect();
    let replayable = |inst: &Inst| {
        is_pure(&inst.kind)
            && inst
                .result
                .is_none_or(|r| !cells.contains(&r) && !operands.contains(&r))
    };
    let pos = fast_pos[&(s.block, s.inst_idx)];
    let head = &func.blocks[s.block.0 as usize].insts[..pos];
    let p_insts = &func.blocks[p.0 as usize].insts;
    (p_insts.iter().all(replayable) && head.iter().all(replayable)).then_some(p)
}
