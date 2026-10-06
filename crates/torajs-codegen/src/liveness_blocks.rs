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

use torajs_core::ssa::id_hash::HashMap;

use torajs_core::ssa::{Function, Terminator};

use crate::liveness::{visit_inst_operands, visit_terminator_operands};

/// Per-block liveness, index-aligned with `func.blocks`.
pub struct BlockLiveness {
    /// Global slot of the block's first inst (== its terminator slot
    /// when the block has no insts).
    pub start: Vec<u32>,
    /// Global slot of the block's terminator.
    pub end: Vec<u32>,
    pub live_in: Vec<ValueSet>,
    pub live_out: Vec<ValueSet>,
}

/// A set of value ids, one bit per value of the function.
#[derive(Clone, PartialEq, Eq)]
pub struct ValueSet {
    words: Vec<u64>,
}

impl ValueSet {
    fn new(values: usize) -> Self {
        ValueSet {
            words: vec![0; values.div_ceil(64)],
        }
    }

    fn insert(&mut self, v: u32) {
        self.words[v as usize / 64] |= 1 << (v % 64);
    }

    pub fn contains(&self, v: u32) -> bool {
        self.words
            .get(v as usize / 64)
            .is_some_and(|w| w & (1 << (v % 64)) != 0)
    }

    /// The members, ascending.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            let mut w = w;
            std::iter::from_fn(move || {
                (w != 0).then(|| {
                    let bit = w.trailing_zeros();
                    w &= w - 1;
                    i as u32 * 64 + bit
                })
            })
        })
    }
}

pub fn compute_block_liveness(func: &Function) -> BlockLiveness {
    let n = func.blocks.len();
    let mut start: Vec<u32> = vec![0; n];
    let mut end: Vec<u32> = vec![0; n];
    let empty = ValueSet::new(func.values.len());
    let mut uses: Vec<ValueSet> = vec![empty.clone(); n];
    let mut defs: Vec<ValueSet> = vec![empty.clone(); n];
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
                if !defs[i].contains(v.0) {
                    uses[i].insert(v.0);
                }
            });
            if let Some(result) = inst.result {
                defs[i].insert(result.0);
            }
            cursor += 1;
        }
        visit_terminator_operands(&block.term, |v| {
            if !defs[i].contains(v.0) {
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

    let mut live_in: Vec<ValueSet> = vec![empty.clone(); n];
    let mut live_out: Vec<ValueSet> = vec![empty; n];
    let words = live_in.first().map_or(0, |s| s.words.len());
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            for w in 0..words {
                let out = succs[b].iter().fold(0, |acc, &s| acc | live_in[s].words[w]);
                let inn = uses[b].words[w] | (out & !defs[b].words[w]);
                changed |= out != live_out[b].words[w] || inn != live_in[b].words[w];
                live_out[b].words[w] = out;
                live_in[b].words[w] = inn;
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
