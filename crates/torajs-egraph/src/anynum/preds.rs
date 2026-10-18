//! The predecessor lists of a function's blocks, shared by the ARC
//! code motion passes ([`super::inc_sink`], [`super::dec_split`]).

use torajs_core::ssa::Function;

use crate::float_demote::cfg::successors;

/// The predecessors of each block, in one array.
pub(super) struct Preds {
    start: Vec<u32>,
    list: Vec<usize>,
}

impl Preds {
    pub(super) fn of(func: &Function) -> Self {
        let n = func.blocks.len();
        let mut start = vec![0u32; n + 1];
        for block in &func.blocks {
            for s in successors(&block.term) {
                start[s.0 as usize + 1] += 1;
            }
        }
        for b in 0..n {
            start[b + 1] += start[b];
        }
        let mut fill = start.clone();
        let mut list = vec![0; start[n] as usize];
        for (b, block) in func.blocks.iter().enumerate() {
            for s in successors(&block.term) {
                let at = &mut fill[s.0 as usize];
                list[*at as usize] = b;
                *at += 1;
            }
        }
        Preds { start, list }
    }

    pub(super) fn of_block(&self, b: usize) -> &[usize] {
        &self.list[self.start[b] as usize..self.start[b + 1] as usize]
    }
}
