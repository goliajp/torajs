//! The parameter hop of the dynobj-degrade scope simulation — sibling
//! of the parent's binding bookkeeping. Child module: reaches the
//! parent's private `Walker` fields directly (walk.rs precedent).
//!
//! A `delete` whose receiver is a PARAMETER of a named function
//! (`function era(h) { delete h[k] }`) degrades nothing where it
//! stands — a param was never a candidate declaration — but the
//! literal it removes from lives at the call site: `var o = {aa: 1};
//! era(o)`. Without this hop `o` stayed a struct, the any-lane delete
//! answered false and removed nothing (the recorded reflection
//! boundary in `torajs-anyvalue/prop_delete.rs`), and a `for-in`
//! over `o` kept visiting the key (test262 `S12.6.4_A7_T1`).
//!
//! Only `delete` triggers through a param. The define family and the
//! expando / computed writes already work on a struct reached through
//! `any` (the cell converts in place; the props dict takes expandos),
//! so widening their call-site literals would cost any-lane precision
//! for nothing.
//!
//! Resolution is the parent's: a Tracked argument marks its
//! declaration sites; a Free one takes the same name-keyed fallback a
//! free `delete` receiver takes; an Opaque one marks nothing. A call
//! that forwards the enclosing fn's own param (`function a(h) { b(h) }
//! function b(h) { delete h.x }`) is an edge, and triggers close over
//! the edges before the call sites are judged. Calls are matched by
//! callee NAME module-wide (the parent's over-broad-never-wrong
//! posture).

use std::collections::HashSet;

use super::{Resolution, Walker, strip_as};
use crate::ast::{Expr, ExprId};

/// `(fn name, param index)` — one parameter slot of a named fn.
type ParamSlot = (String, usize);

/// What a call site's argument resolved to when the call was walked.
enum Pending {
    Tracked(Vec<ExprId>),
    Free(String),
}

#[derive(Default)]
pub(super) struct ParamHop {
    /// Param slots whose body deletes through the param.
    triggers: HashSet<ParamSlot>,
    /// `(callee slot, enclosing-fn param slot)` — the callee's param
    /// receives the enclosing fn's param verbatim.
    forwards: Vec<(ParamSlot, ParamSlot)>,
    /// Every call `g(.., arg_i, ..)` with a binding-shaped argument.
    pending: Vec<(ParamSlot, Pending)>,
}

impl ParamHop {
    pub(super) fn mark_trigger(&mut self, cur_fn: Option<&str>, idx: usize) {
        if let Some(f) = cur_fn {
            self.triggers.insert((f.to_string(), idx));
        }
    }

    /// Close the triggers over the forwarding edges, then judge every
    /// recorded call site. Runs after the walk, before the parent's
    /// name-keyed fallbacks are applied (it feeds one of them).
    pub(super) fn apply(
        self,
        out: &mut HashSet<ExprId>,
        fallback_write_names: &mut HashSet<String>,
    ) {
        let mut triggers = self.triggers;
        loop {
            let mut changed = false;
            for (callee_slot, param_slot) in &self.forwards {
                if triggers.contains(callee_slot) && triggers.insert(param_slot.clone()) {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (slot, pending) in self.pending {
            if !triggers.contains(&slot) {
                continue;
            }
            match pending {
                Pending::Tracked(eids) => out.extend(eids),
                Pending::Free(name) => {
                    fallback_write_names.insert(name);
                }
            }
        }
    }
}

impl Walker<'_> {
    /// Record a call by bare name whose arguments are bindings.
    pub(super) fn scan_named_call(&mut self, callee: ExprId, args: &[ExprId]) {
        let ast = self.view;
        let Expr::Ident(g) = ast.get_expr(callee) else {
            return;
        };
        for (i, a) in args.iter().enumerate() {
            let Expr::Ident(recv) = ast.get_expr(strip_as(ast, *a)) else {
                continue;
            };
            let slot = (g.clone(), i);
            match self.resolve(recv) {
                Resolution::Tracked(eids) => self.hop.pending.push((slot, Pending::Tracked(eids))),
                Resolution::Free => self.hop.pending.push((slot, Pending::Free(recv.clone()))),
                Resolution::Param(j) => {
                    if let Some(f) = &self.cur_fn {
                        self.hop.forwards.push((slot, (f.clone(), j)));
                    }
                }
                Resolution::Opaque => {}
            }
        }
    }
}
