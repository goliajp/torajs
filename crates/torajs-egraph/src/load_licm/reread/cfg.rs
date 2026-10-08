//! The control-flow questions `reread` asks: where the loop's
//! preheader is (making one when the loop has several ways in), and
//! which header-word reads may hand their uses the register cell
//! itself rather than a copy of it.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Block, BlockId, Function, Operand, Terminator, ValueId};

use super::LoadRereadStats;

/// The loop's preheader: its single way in from outside when that is a
/// plain jump, otherwise a new block every outside edge now goes
/// through.
pub(super) fn preheader(
    func: &mut Function,
    header: BlockId,
    body: &HashSet<BlockId>,
    stats: &mut LoadRereadStats,
) -> BlockId {
    let outside: Vec<BlockId> = (0..func.blocks.len() as u32)
        .map(BlockId)
        .filter(|b| !body.contains(b) && succs(&func.blocks[b.0 as usize].term).contains(&header))
        .collect();
    if let [p] = outside.as_slice()
        && matches!(func.blocks[p.0 as usize].term, Terminator::Br(_))
    {
        return *p;
    }
    let pre = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id: pre,
        insts: Vec::new(),
        term: Terminator::Br(header),
    });
    for b in outside {
        match &mut func.blocks[b.0 as usize].term {
            Terminator::Br(t) => *t = pre,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                for t in [then_blk, else_blk] {
                    if *t == header {
                        *t = pre;
                    }
                }
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    stats.preheaders_made += 1;
    pre
}

pub(super) fn succs(term: &Terminator) -> Vec<BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

/// Blocks reachable from `from`'s successors without passing through
/// `header` (the next iteration starts there).
fn reach(func: &Function, from: &[BlockId], header: BlockId) -> HashSet<BlockId> {
    let mut seen: HashSet<BlockId> = HashSet::default();
    let mut work: Vec<BlockId> = from
        .iter()
        .flat_map(|b| succs(&func.blocks[b.0 as usize].term))
        .collect();
    while let Some(b) = work.pop() {
        if b == header || !seen.insert(b) {
            continue;
        }
        work.extend(succs(&func.blocks[b.0 as usize].term));
    }
    seen
}

/// Of the header-word reads `reads` (result → (block, index)), those
/// whose every use sees the cell holding the value the read would
/// have produced: no re-read (`rereads`: block → indexes of the
/// kernel calls followed by one) lies after the read in its block, or
/// on a path from it to a use within the same iteration. Their uses
/// can name the cell directly; the others keep `r = copy cell`.
pub(super) fn direct_reads(
    func: &Function,
    header: BlockId,
    reads: &HashMap<ValueId, (BlockId, usize)>,
    rereads: &HashMap<BlockId, Vec<usize>>,
) -> HashSet<ValueId> {
    let mut uses: HashMap<ValueId, Vec<BlockId>> = HashMap::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        let b = BlockId(bi as u32);
        for inst in &block.insts {
            torajs_core::ssa::visit_value_operands(&inst.kind, |v| {
                if reads.contains_key(&v) {
                    uses.entry(v).or_default().push(b);
                }
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &block.term
            && reads.contains_key(v)
        {
            uses.entry(*v).or_default().push(b);
        }
    }
    let kernel_blocks: Vec<BlockId> = rereads.keys().copied().collect();
    reads
        .iter()
        .filter(|(r, (d, i))| {
            if rereads.get(d).is_some_and(|ks| ks.iter().any(|k| k > i)) {
                return false;
            }
            let after_def = reach(func, &[*d], header);
            let hit: Vec<BlockId> = kernel_blocks
                .iter()
                .copied()
                .filter(|k| after_def.contains(k))
                .collect();
            let mut tainted = reach(func, &hit, header);
            tainted.extend(hit);
            uses.get(r)
                .is_none_or(|us| us.iter().all(|u| u == d || !tainted.contains(u)))
        })
        .map(|(r, _)| *r)
        .collect()
}
