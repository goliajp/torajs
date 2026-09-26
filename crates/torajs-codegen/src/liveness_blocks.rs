//! Block-level live-in / live-out sets — the textbook backward
//! data-flow fix-point over the block CFG:
//!
//!   live_in[B]  = use[B] ∪ (live_out[B] \ def[B])
//!   live_out[B] = ∪{live_in[s] : s ∈ succ(B)}
//!
//! Positions use the global inst-slot numbering of
//! `liveness::compute_intervals`: one index per inst, then one for the
//! terminator. Interval extension and copy coalescing both read the
//! result.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, Terminator};

use crate::liveness::{visit_inst_operands, visit_terminator_operands};

/// Per-block liveness, index-aligned with `func.blocks`.
pub struct BlockLiveness {
    /// Global slot of the block's first inst (== its terminator slot
    /// when the block has no insts).
    pub start: Vec<u32>,
    /// Global slot of the block's terminator.
    pub end: Vec<u32>,
    pub live_in: Vec<HashSet<u32>>,
    pub live_out: Vec<HashSet<u32>>,
}

pub fn compute_block_liveness(func: &Function) -> BlockLiveness {
    let n = func.blocks.len();
    let mut start: Vec<u32> = vec![0; n];
    let mut end: Vec<u32> = vec![0; n];
    let mut uses: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut defs: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];

    // BlockId may be non-contiguous, so map it to a position.
    let pos: HashMap<u32, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.0, i))
        .collect();

    let mut cursor: u32 = 0;
    for (i, block) in func.blocks.iter().enumerate() {
        start[i] = cursor;
        for inst in &block.insts {
            visit_inst_operands(&inst.kind, |v| {
                if !defs[i].contains(&v.0) {
                    uses[i].insert(v.0);
                }
            });
            if let Some(result) = inst.result {
                defs[i].insert(result.0);
            }
            cursor += 1;
        }
        visit_terminator_operands(&block.term, |v| {
            if !defs[i].contains(&v.0) {
                uses[i].insert(v.0);
            }
        });
        end[i] = cursor;
        cursor += 1;

        match &block.term {
            Terminator::Br(t) => succs[i].extend(pos.get(&t.0)),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                succs[i].extend(pos.get(&then_blk.0));
                succs[i].extend(pos.get(&else_blk.0));
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }

    let mut live_in: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    let mut live_out: Vec<HashSet<u32>> = vec![HashSet::new(); n];
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let mut new_out: HashSet<u32> = HashSet::new();
            for &s in &succs[b] {
                new_out.extend(live_in[s].iter().copied());
            }
            if new_out != live_out[b] {
                live_out[b] = new_out;
                changed = true;
            }
            let mut new_in = uses[b].clone();
            new_in.extend(live_out[b].iter().filter(|v| !defs[b].contains(v)));
            if new_in != live_in[b] {
                live_in[b] = new_in;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    BlockLiveness {
        start,
        end,
        live_in,
        live_out,
    }
}
