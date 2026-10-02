//! Where a dead attempt sends the scan.
//!
//! An attempt that began on state 0's edge and died there moves the
//! scan one unit. Any other death goes through `dead`, and from there
//! the scan moves one unit past the start — or, for a matcher with a
//! leading run state, one unit past the position the run ended at:
//! every start inside the run would have reached the run state at
//! that same position and died the same way.

use crate::ssa::{BlockId, Operand, Terminator, ValueId};

use super::Emit;

impl Emit<'_> {
    /// The block a variant of state `i` falls to when no edge takes
    /// the unit at `pos`.
    pub(super) fn fail_block(&mut self, i: usize, pos: ValueId) -> BlockId {
        if self.fresh_on_edge(i) {
            return self.next_st;
        }
        let Some((_, slot)) = self.resume.filter(|&(run, _)| run == i) else {
            return self.dead;
        };
        let blk = self.f.add_block();
        self.store(blk, Operand::Value(pos), slot, 0);
        self.f.set_term(blk, Terminator::Br(self.dead));
        blk
    }

    /// An edge of state `i` to `target` taken at `pos`: leaving the
    /// run state records where the run ended.
    pub(super) fn note_run_end(&mut self, blk: BlockId, i: usize, target: usize, pos: ValueId) {
        match self.resume {
            Some((run, slot)) if run == i && target != i => {
                self.store(blk, Operand::Value(pos), slot, 0);
            }
            _ => {}
        }
    }

    /// The block `dead` continues the scan in when no accept answers:
    /// `next_st` itself, or a block that first moves the start to the
    /// end of the run.
    pub(super) fn resume_scan(&mut self) -> BlockId {
        let Some((_, slot)) = self.resume else {
            return self.next_st;
        };
        let blk = self.f.add_block();
        let end = self.load(blk, slot, 0);
        self.store(blk, Operand::Value(end), self.st_slot, 0);
        self.f.set_term(blk, Terminator::Br(self.next_st));
        blk
    }
}
