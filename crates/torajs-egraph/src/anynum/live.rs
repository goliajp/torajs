//! Backward liveness over a function: the values live into each
//! block. A side exit writes back what its target reads.
//!
//! One bit per value and block: a block's reads before its own writes
//! and its writes are taken once, and the rounds only or words
//! together until nothing changes.

use torajs_core::ssa::{BlockId, Function, ValueId};

use crate::float_demote::cfg::{successors, term_operand_value};
use crate::rc_peephole::visit_value_operands;

pub(super) struct LiveIn {
    words: usize,
    bits: Vec<u64>,
}

impl LiveIn {
    /// The values live into `b`, in id order.
    pub(super) fn of(&self, b: BlockId) -> Vec<ValueId> {
        let row = &self.bits[b.0 as usize * self.words..][..self.words];
        let mut out = Vec::new();
        for (wi, &word) in row.iter().enumerate() {
            let mut w = word;
            while w != 0 {
                out.push(ValueId((wi * 64) as u32 + w.trailing_zeros()));
                w &= w - 1;
            }
        }
        out
    }
}

/// Live-in sets of a function whose block ids are their indices.
pub(super) fn live_in(func: &Function) -> LiveIn {
    let words = func.values.len().div_ceil(64);
    let n = func.blocks.len();
    let set = |row: &mut [u64], v: ValueId| row[v.0 as usize / 64] |= 1 << (v.0 % 64);
    let has = |row: &[u64], v: ValueId| row[v.0 as usize / 64] & (1 << (v.0 % 64)) != 0;
    // per block: what it reads before writing, and what it writes
    let mut reads = vec![0u64; n * words];
    let mut writes = vec![0u64; n * words];
    for (bi, block) in func.blocks.iter().enumerate() {
        let r = &mut reads[bi * words..][..words];
        let w = &mut writes[bi * words..][..words];
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if !has(w, v) {
                    set(r, v);
                }
            });
            if let Some(d) = inst.result {
                set(w, d);
            }
        }
        if let Some(v) = term_operand_value(&block.term)
            && !has(w, v)
        {
            set(r, v);
        }
    }
    let mut bits = reads.clone();
    let mut out = vec![0u64; words];
    let mut changed = true;
    while changed {
        changed = false;
        for bi in (0..n).rev() {
            out.fill(0);
            for s in successors(&func.blocks[bi].term) {
                let row = &bits[s.0 as usize * words..][..words];
                for (o, x) in out.iter_mut().zip(row) {
                    *o |= x;
                }
            }
            let at = bi * words;
            for k in 0..words {
                let v = reads[at + k] | (out[k] & !writes[at + k]);
                if v != bits[at + k] {
                    bits[at + k] = v;
                    changed = true;
                }
            }
        }
    }
    LiveIn { words, bits }
}
