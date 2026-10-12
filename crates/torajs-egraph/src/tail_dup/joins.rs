//! Which joins get copies, and for which arms, read off the graph
//! before any copying.

use super::{MAX_TAIL_INSTS, duplicable, targets};
use crate::block_chain::{ChainFacts, DomView};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;
use torajs_core::ssa::{BlockId, Function, Terminator};

/// A join worth copying: its id, its tail, its loop's header and the
/// arms that get a copy.
pub(super) type Join = (u32, Vec<u32>, u32, Vec<u32>);

/// Every join worth copying, and the chainer's facts if reading the
/// joins needed them. Tails and arms are read off the original graph:
/// copying one join retargets an arm that may be the head of another's
/// tail.
pub(super) fn joins_to_copy<'d>(
    func: &Function,
    dom: &'d DominatorTree,
    la: &LoopAnalysis,
) -> (Vec<Join>, Option<ChainFacts<'d>>) {
    // the predecessors, in block order, of every block that could head
    // a tail (an empty list allocates nothing)
    let heads: Vec<bool> = func.blocks.iter().map(duplicable).collect();
    let mut preds: Vec<Vec<u32>> = vec![Vec::new(); func.blocks.len()];
    for block in &func.blocks {
        for t in targets(&block.term).filter(|&t| heads[t as usize]) {
            preds[t as usize].push(block.id.0);
        }
    }
    // the innermost loop each block belongs to: nested bodies are
    // smaller, so the bodies are marked largest first and the smallest
    // one holding a block marks it last
    let loops = la.loops();
    let mut by_size: Vec<usize> = (0..loops.len()).collect();
    by_size.sort_by_key(|&i| std::cmp::Reverse(loops[i].body.len()));
    let mut innermost: Vec<Option<usize>> = vec![None; func.blocks.len()];
    let mut headers = vec![false; func.blocks.len()];
    for i in by_size {
        for b in &loops[i].body {
            innermost[b.0 as usize] = Some(i);
        }
        headers[loops[i].header.0 as usize] = true;
    }
    // the guards' cold arms, read only once a join qualifies
    let facts = std::cell::OnceCell::new();
    let mut joins: Vec<Join> = Vec::new();
    for block in &func.blocks {
        let id = block.id.0;
        if headers[id as usize] {
            continue;
        }
        let Some(lp) = innermost[id as usize].map(|i| &loops[i]) else {
            continue;
        };
        let in_body = |b: u32| lp.contains(&BlockId(b));
        let ps = &preds[id as usize];
        // every arm comes from inside the loop, or the join is an entry
        if ps.len() < 2 || ps.iter().any(|&p| !in_body(p)) {
            continue;
        }
        // the copy pays for itself only where the tail ends in the
        // back edge: the original pays that taken branch too
        let tail = tail_from(func, id, &in_body, &headers);
        let Some(&last) = tail.last() else { continue };
        if !matches!(func.blocks[last as usize].term, Terminator::Br(t) if t == lp.header) {
            continue;
        }
        // the arms that jump into the join: an arm that reaches it by
        // a conditional branch pays nothing for the edge, and a cold
        // arm keeps the shared tail; those keep the original
        // reachable, and where there are none the first jump does
        let jumps: Vec<u32> = ps
            .iter()
            .copied()
            .filter(|&p| matches!(func.blocks[p as usize].term, Terminator::Br(_)))
            .filter(|&p| {
                !facts
                    .get_or_init(|| ChainFacts::new(func, DomView::whole(dom)))
                    .cold_path(func, &in_body, p)
            })
            .collect();
        let keep = usize::from(jumps.len() == ps.len());
        if jumps.len() > keep {
            joins.push((id, tail, lp.header.0, jumps[keep..].to_vec()));
        }
    }
    (joins, facts.into_inner())
}

/// The run of small `br`-ending blocks from `start` on, inside `body`,
/// up to the size cap: each follows the one before as its only
/// successor, and a loop header or a block seen twice ends the run.
/// Empty when `start` itself does not qualify.
fn tail_from(
    func: &Function,
    start: u32,
    in_body: &impl Fn(u32) -> bool,
    headers: &[bool],
) -> Vec<u32> {
    let mut tail: Vec<u32> = Vec::new();
    let mut insts = 0;
    let mut cur = start;
    while !headers[cur as usize]
        && in_body(cur)
        && !tail.contains(&cur)
        && duplicable(&func.blocks[cur as usize])
        && insts + func.blocks[cur as usize].insts.len() <= MAX_TAIL_INSTS
    {
        insts += func.blocks[cur as usize].insts.len();
        tail.push(cur);
        let Terminator::Br(next) = func.blocks[cur as usize].term else {
            break;
        };
        cur = next.0;
    }
    tail
}
