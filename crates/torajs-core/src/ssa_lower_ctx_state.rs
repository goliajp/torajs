//! Small `LowerCtx<'a>` ctx-state query helpers extracted from
//! `ssa_lower.rs` chunk 402 — Path A.3-batch23.
//!
//! Three tiny leaf methods that inspect or route based on the
//! current lowering context (main-fn vs. nested fn / block open
//! status / while-loop fast-path):
//!
//! - `num_width_local_key(name) -> SlotKey` — W1 map key for a let
//!   binding: `Global` for top-level bindings (regardless of Pass
//!   1.5 promotion), `Local` for nested-fn locals.
//! - `cur_open() -> bool` — true iff current block still has the
//!   default `Unreachable` terminator (used by caller sites after
//!   lowering a sub-statement to decide whether a fall-through Br
//!   is still needed).
//! - `try_lower_while_fast(prev, s) -> bool` — 12-c-1 route through
//!   `lower_while_inner` with the let-zero counter derived from
//!   `prev`; returns true iff `s` was a While that got fast-lowered.

use crate::ast::Stmt;
use crate::ssa::Terminator;
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_push_loop_detect::let_counter_zero_name;
use crate::ssa_lower_while_push_fast::lower_while_inner;

impl<'a> LowerCtx<'a> {
    /// W1 — the num_width SlotKey for a let binding in the current fn.
    /// Top-level bindings key as Global regardless of whether Pass 1.5
    /// promoted them (the analysis keys every top-level let that way).
    pub(crate) fn num_width_local_key(&self, name: &str) -> crate::num_width::SlotKey {
        if self.is_main_fn {
            crate::num_width::SlotKey::Global(name.to_string())
        } else {
            crate::num_width::SlotKey::Local(self.f.name.clone(), name.to_string())
        }
    }

    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    pub(crate) fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    /// 12-c-1 — route `while` through [`lower_while_inner`] with the
    /// let-zero counter derived from `prev`. Returns `true` iff `s`
    /// was a While; caller lowers non-Whiles normally. See the module
    /// doc on [`crate::ssa_lower_while_push_fast`].
    pub(crate) fn try_lower_while_fast(&mut self, prev: Option<&Stmt>, s: &Stmt) -> bool {
        let Stmt::While { cond, body } = s else {
            return false;
        };
        let counter = let_counter_zero_name(self.ast, prev);
        lower_while_inner(self, *cond, body, counter.as_deref());
        true
    }
}
