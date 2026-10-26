//! The blocks and values the conversions of one formation round
//! rewrote.

use torajs_core::ssa::{Function, ValueId};

/// What the conversions of one round rewrote: the counts the round
/// started from are stale for these, so a candidate reading any of
/// them waits for the next round, whose counts see the rewrite.
pub(super) struct Touched {
    blocks: Vec<bool>,
    values: Vec<bool>,
}

impl Touched {
    pub(super) fn new(func: &Function) -> Self {
        Self {
            blocks: vec![false; func.blocks.len()],
            values: vec![false; func.values.len()],
        }
    }

    /// A value made this round (past the round's start) counts as
    /// rewritten.
    pub(super) fn any(&self, blocks: &[usize], values: impl IntoIterator<Item = ValueId>) -> bool {
        blocks.iter().any(|&b| self.blocks[b])
            || values
                .into_iter()
                .any(|v| self.values.get(v.0 as usize).is_none_or(|&t| t))
    }

    pub(super) fn mark(&mut self, blocks: &[usize], values: impl IntoIterator<Item = ValueId>) {
        for &b in blocks {
            self.blocks[b] = true;
        }
        for v in values {
            if let Some(t) = self.values.get_mut(v.0 as usize) {
                *t = true;
            }
        }
    }
}
