//! The constraints in force at the current point of the block the
//! ascent is walking. A block's stored list is read in place until a
//! merge placeholder needs its range or a cell def kills an entry;
//! only then is it copied into the buffer every block shares.

use torajs_core::ssa::{BlockId, Operand, ValueId};

use super::merge::Inherited;
use super::refine::Constraint;
use super::transfer::NumFact;

pub(super) struct Live<'a> {
    inherited: &'a Inherited,
    block: BlockId,
    /// the list in force when `owned`
    buf: Vec<Constraint>,
    owned: bool,
    /// `named[v] == epoch`: the list may speak of `v`; one it never
    /// names reads as its plain state
    named: Vec<u32>,
    epoch: u32,
}

impl<'a> Live<'a> {
    pub(super) fn new(inherited: &'a Inherited, values: usize) -> Self {
        Live {
            inherited,
            block: BlockId(0),
            buf: Vec::new(),
            owned: false,
            named: vec![0; values],
            epoch: 0,
        }
    }

    /// Start block `b` with its entry constraints, each merge
    /// placeholder read as the range `ranges` gives its join.
    pub(super) fn enter(&mut self, b: BlockId, ranges: &[Option<NumFact>]) {
        self.block = b;
        self.owned = self.inherited.has_merges(b);
        if self.owned {
            self.inherited.resolve(b, ranges, &mut self.buf);
        }
        self.epoch += 1;
        let list = if self.owned {
            &self.buf
        } else {
            self.inherited.list(b)
        };
        for c in list {
            self.named[c.value().0 as usize] = self.epoch;
        }
    }

    pub(super) fn all(&self) -> &[Constraint] {
        if self.owned {
            &self.buf
        } else {
            self.inherited.list(self.block)
        }
    }

    /// What a lookup of `op` reads: the whole list, or nothing when
    /// the list never names the value.
    pub(super) fn for_operand(&self, op: &Operand) -> &[Constraint] {
        match op {
            Operand::Value(v) if self.named[v.0 as usize] != self.epoch => &[],
            _ => self.all(),
        }
    }

    pub(super) fn is_even(&self, op: &Operand) -> bool {
        matches!(op, Operand::Value(v)
            if self.all().iter().any(|c| matches!(c, Constraint::Even(e) if e == v)))
    }

    /// Cell `r` was redefined: what the list said about its previous
    /// contents no longer applies below.
    pub(super) fn kill(&mut self, r: ValueId) {
        let killed = |c: &Constraint| c.killed_by(|v| v == r);
        if self.owned {
            self.buf.retain(|c| !killed(c));
        } else if self.inherited.list(self.block).iter().any(killed) {
            let list = self.inherited.list(self.block);
            self.buf.clear();
            self.buf.extend(list.iter().filter(|c| !killed(c)).copied());
            self.owned = true;
        }
    }
}
