//! Facts the lowering establishes while it emits and spends later in
//! the same fn.
//!
//! - A binding whose value passed a nullish guard holds a live cell
//!   from that point on, until something writes it: the member reads
//!   after the first one on `const t = xs[i]` need no second test.
//!   The fact is flow-sensitive and kept conservatively — a nested
//!   region (branch arm, loop, try, switch clause) returns only the
//!   facts that held before it, minus any it killed; a loop forgets
//!   everything at entry (a body write reaches the next iteration's
//!   reads); a sub-expression that runs on one branch of a join
//!   records nothing.
//! - An expression the producing lane proved non-nullish (a fused
//!   element read that throws where the plain read answers
//!   `undefined`).
//! - The whole-program answer to "can any array carry a shifted deque
//!   head", from [`crate::ssa_lower_pass_0_5::arr_head_census`].
//! - The receivers of a constructor's leading `this.f = v` writes,
//!   where the instance the factory just minted has not escaped yet
//!   (`ssa_lower_fn::fresh_this`): nothing can have frozen it or
//!   redefined a field, so the store needs no writable guard, and a
//!   field's first write finds the factory's nullish default.

use std::collections::{HashMap, HashSet};

use crate::ast::ExprId;

#[derive(Default)]
pub(crate) struct Proofs {
    /// No array in the program ever has a non-zero deque head, so
    /// every element lives at `data + idx * stride`.
    pub(crate) arr_head_pinned: bool,
    /// Index reads that are the receiver of a member read of the named
    /// field; the typed lane fuses the bounds / hole exits into the
    /// TypeError that read would raise anyway.
    pub(crate) recv_field: HashMap<ExprId, String>,
    /// Expressions the lane that produced them proved non-nullish.
    pub(crate) nonnull_exprs: HashSet<ExprId>,
    /// `this` receivers of a ctor's field writes reached before the
    /// instance escapes: unfrozen, no field redefined.
    pub(crate) fresh_this_recv: HashSet<ExprId>,
    /// The subset whose field holds the factory's null / undefined
    /// default at that point, so there is no old value to release.
    pub(crate) fresh_this_first_write: HashSet<ExprId>,
    /// Inits of borrow-shaped lets whose binding is read after
    /// something may have released its source
    /// (`crate::ssa_lower::alias_hazard`): they bind owned.
    pub(crate) released_alias_inits: HashSet<ExprId>,
    /// Element stores `xs[i] = s` that may take the `const` binding's
    /// own stake: the rest of its block only reads `s`
    /// (`crate::ssa_lower_index_assign::move_const_temp`).
    pub(crate) slot_moves: HashMap<ExprId, String>,
    /// Fn-local bindings whose current value passed a nullish guard on
    /// every path reaching the point being lowered.
    nonnull_lets: HashSet<String>,
    /// > 0 while lowering a sub-expression that runs on one branch of
    /// > a join (`a && b`, `c ? x : y`, `o?.p`, `a ?? b`).
    cond_depth: u32,
}

impl Proofs {
    pub(crate) fn new(arr_head_pinned: bool) -> Self {
        Proofs {
            arr_head_pinned,
            ..Default::default()
        }
    }

    pub(crate) fn let_is_nonnull(&self, name: &str) -> bool {
        self.nonnull_lets.contains(name)
    }

    /// A guard on `name` just ran on the straight-line path.
    pub(crate) fn note_let_nonnull(&mut self, name: &str) {
        if self.cond_depth == 0 {
            self.nonnull_lets.insert(name.to_string());
        }
    }

    /// `name` is (re)bound or written: whatever was proven about its
    /// old value is gone.
    pub(crate) fn kill(&mut self, name: &str) {
        self.nonnull_lets.remove(name);
    }

    /// Loop entry: a write anywhere in the loop reaches the reads
    /// before it on the next iteration.
    pub(crate) fn forget_lets(&mut self) {
        self.nonnull_lets.clear();
    }

    /// Snapshot before a nested region; pair with [`Self::region_leave`].
    pub(crate) fn region_enter(&self) -> HashSet<String> {
        self.nonnull_lets.clone()
    }

    /// Back on the path around the region: keep a fact only if it held
    /// before the region and the region did not kill it.
    pub(crate) fn region_leave(&mut self, saved: &HashSet<String>) {
        self.nonnull_lets.retain(|n| saved.contains(n));
    }

    pub(crate) fn enter_cond(&mut self) {
        self.cond_depth += 1;
    }

    pub(crate) fn leave_cond(&mut self) {
        self.cond_depth -= 1;
    }
}
