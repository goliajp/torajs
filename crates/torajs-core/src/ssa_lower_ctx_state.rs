//! Small `LowerCtx<'a>` ctx-state query helpers extracted from
//! `ssa_lower.rs` chunk 402 — Path A.3-batch23.
//!
//! Leaf methods that inspect or route based on the current lowering
//! context (main-fn vs. nested fn / block open status / while-loop
//! fast-path), plus the shared pre-body binding-set prime:
//!
//! - `prime_body_binding_sets(body)` — escape-capture / deque /
//!   escape-obj set prime shared by `lower_fn` and
//!   `synthesize_main` (deduped from their identical walks).
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
    /// Prime the per-body binding sets shared by `lower_fn` and
    /// `synthesize_main` before any statement lowers:
    /// escape-captured lets (T-15.g.5 — a top-level `let` captured by
    /// a later closure must not stack-alloc), the mutated-capture
    /// mirror, deque-unsafe Array bindings (11-A1) and escape-bound
    /// Obj bindings (11-A2-a).
    ///
    /// Returns the body-scoped assigned-name set so `lower_fn` can
    /// hand it to `materialize_fn_params`: a non-Copy param that the
    /// body reassigns must copy-in an owned stake (the reassignment
    /// clears `moved` at compile time while firing only on a runtime
    /// branch — a default-param guard — so the borrow convention
    /// cannot stay balanced on the explicit-argument path).
    pub(crate) fn prime_body_binding_sets<'s>(
        &mut self,
        body: impl Iterator<Item = &'s Stmt> + Clone,
    ) -> std::collections::HashSet<String> {
        let mut scan = crate::ssa_lower_closure_captures::ClosureScan::default();
        for s in body.clone() {
            crate::ssa_lower_closure_captures::collect_closure_captures_in_stmt(
                self.ast, s, &mut scan,
            );
        }
        self.escape_captured_lets.extend(scan.captures);
        // Scoped to this body + the closures it constructs — a
        // same-named assignment in an unrelated scope must not
        // promote this fn's never-written binding to a capture
        // box (RFC 20260731-mono-closure-clone 刀 2).
        let assigned = crate::ssa_lower_closure_captures::collect_assigned_names_scoped(
            self.ast,
            body.clone(),
            &scan.fn_names,
        );
        if !self.escape_captured_lets.is_empty() {
            self.mutated_captured_lets = assigned.clone();
        }
        for s in body.clone() {
            crate::ssa_lower_deque_escape::collect_deque_arr_names_in_stmt(
                self.ast,
                s,
                &mut self.deque_arrs,
            );
        }
        for s in body {
            crate::ssa_lower_obj_escape::collect_escape_obj_let_names_in_stmt(
                self.ast,
                s,
                &mut self.escape_obj_lets,
            );
        }
        assigned
    }

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
