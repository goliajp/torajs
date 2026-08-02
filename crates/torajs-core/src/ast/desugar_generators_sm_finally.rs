//! Generator try/finally lowering — RFC 20260802 D1 + D3a.
//!
//! Split out of `desugar_generators_sm_try` when the D3a return
//! routing pushed that file past the 500-line HARD limit. Owns the
//! per-exit-path finally duplication (`lower_try_finally`), the
//! return-through-finally routing (frame stack + placeholder-patched
//! gotos), and the conservative gate walker. The catch-region arm,
//! dispatch wrap, and throw-injection stay in the `_sm_try` sibling.

use super::desugar_generators_sm::{DISPATCH_LABEL, GenSm, RESUME_LOCAL};
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::desugar_generators_sm_try::TryRegion;
use super::{Expr, ExprId, Stmt};

/// D3a — one enclosing try/finally frame. A `return v` under it
/// stores v into `this.<slot>` and gotos F's return copy; the copy's
/// state number is unknown until the body finishes lowering, so each
/// goto's Number literal is recorded here and patched afterwards.
///
/// D4 — a `break` / `continue` under it (bare or labeled) whose
/// target loop sits OUTSIDE the try (target state < `try_entry` —
/// states are allocated monotonically, so a loop entered before the
/// try has smaller states) must run F on the way out too: it gotos
/// a per-(kind, label) F copy whose terminal RE-LOWERS the jump,
/// chaining outer frames exactly like the return copy.
pub(super) struct FinallyRetFrame {
    pub(super) try_entry: usize,
    pub(super) slot: String,
    pub(super) gotos: Vec<super::ExprId>,
    /// Escaping-jump placeholders keyed by (is_break, label): one F
    /// copy is minted per distinct key, its terminal re-lowering
    /// exactly that jump.
    pub(super) jump_gotos: Vec<((bool, Option<String>), super::ExprId)>,
}

impl GenSm<'_> {
    /// D3a — route a `return v` out through every enclosing finally:
    /// store v into the innermost frame's slot, goto its (to-be-
    /// patched) return-copy entry, and seal the arm. The raw
    /// `Continue` is pushed directly (NOT via `lower`) so an
    /// enclosing yield-loop's Continue arm can't reinterpret it.
    pub(super) fn emit_return_through_finally(&mut self, v: Option<super::ExprId>) {
        let val = v.unwrap_or_else(|| self.ast.add_expr(Expr::Ident("undefined".into())));
        let mut stmts = self.build_finally_ret_stmts(val);
        self.cur_buf.append(&mut stmts);
        self.flush_cur();
        let dead = self.alloc_state();
        self.cur_state = dead;
    }

    /// Shared builder for the arm-level route above and the inline
    /// rewrite in `rewrite_nested_returns`: `[this.<slot> = val;
    /// __gen_st = <placeholder>; continue __sm;]`. The placeholder
    /// literal is recorded on the innermost frame for the patch. The
    /// labeled continue reaches the dispatch from any nesting depth,
    /// so a return inside an inline inner loop routes correctly
    /// ([`DISPATCH_LABEL`]).
    pub(super) fn build_finally_ret_stmts(&mut self, val: super::ExprId) -> Vec<Stmt> {
        let frame = self.finally_ret.last().expect("caller checked");
        let slot = frame.slot.clone();
        let this_id = self.ast.add_expr(Expr::This);
        let m = self.ast.add_expr(Expr::Member {
            obj: this_id,
            name: slot,
        });
        let store = self.ast.add_expr(Expr::Assign {
            target: m,
            value: val,
        });
        let st = self.ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let placeholder = self.ast.add_expr(Expr::Number(0.0));
        let goto_assign = self.ast.add_expr(Expr::Assign {
            target: st,
            value: placeholder,
        });
        self.finally_ret
            .last_mut()
            .expect("caller checked")
            .gotos
            .push(placeholder);
        vec![
            Stmt::Expr(store),
            Stmt::Expr(goto_assign),
            Stmt::Continue(Some(DISPATCH_LABEL.into())),
        ]
    }

    /// D4 — does a jump to `target`'s state escape the innermost
    /// finally frame? States are allocated monotonically, so a loop
    /// entered BEFORE the try has all its jump-target states below
    /// `try_entry`; a loop opened inside the try sits above it and
    /// its jumps never route.
    pub(super) fn jump_escapes_finally(&self, target: usize) -> bool {
        self.finally_ret
            .last()
            .is_some_and(|f| target < f.try_entry)
    }

    /// D4 — the routed form of a `break` / `continue` (bare or
    /// labeled) whose target loop sits outside the innermost finally
    /// frame: `[__gen_st = <placeholder>; continue __sm;]`, recorded
    /// on the frame's (kind, label)-keyed goto list for the patch.
    /// The dual of [`Self::build_finally_ret_stmts`] minus the value
    /// stash.
    pub(super) fn build_finally_jump_stmts(
        &mut self,
        want_break: bool,
        label: Option<String>,
    ) -> Vec<Stmt> {
        let st = self.ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let placeholder = self.ast.add_expr(Expr::Number(0.0));
        let goto_assign = self.ast.add_expr(Expr::Assign {
            target: st,
            value: placeholder,
        });
        let frame = self.finally_ret.last_mut().expect("caller checked");
        frame.jump_gotos.push(((want_break, label), placeholder));
        vec![
            Stmt::Expr(goto_assign),
            Stmt::Continue(Some(DISPATCH_LABEL.into())),
        ]
    }

    /// D4 — mint one F copy per distinct (kind, label) the body
    /// routed. Each copy's terminal RE-LOWERS its jump with this
    /// frame already popped: the Break/Continue arm consults the
    /// (unchanged) loop stack again and either gotos the real target
    /// or routes through the next enclosing frame — nested finally
    /// chains run inside-out for free, the D3a return-copy pattern.
    fn mint_jump_copies(&mut self, f: &[Stmt], jump_gotos: Vec<((bool, Option<String>), ExprId)>) {
        let mut grouped: Vec<((bool, Option<String>), Vec<ExprId>)> = Vec::new();
        for (key, eid) in jump_gotos {
            match grouped.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(eid),
                None => grouped.push((key, vec![eid])),
            }
        }
        for ((is_break, label), gotos) in grouped {
            let entry = self.alloc_state();
            self.cur_state = entry;
            self.lower_seq(f.to_vec());
            let jump = if is_break {
                Stmt::Break(label)
            } else {
                Stmt::Continue(label)
            };
            self.lower(jump);
            self.flush_cur();
            for eid in gotos {
                self.ast.exprs[eid.0 as usize] = Expr::Number(entry as f64);
            }
        }
    }

    /// D1 — `try { B } finally { F }` via finally duplication (the
    /// javac lowering): one copy of F per exit path. Normal completion
    /// runs the first copy and moves on; an exception from B routes
    /// through the region to the second copy, which rethrows the
    /// preserved exception after F completes. Both copies sit OUTSIDE
    /// the region, so an exception inside F replaces the completion
    /// and propagates (§14.13.3), and a `throw()` injected while
    /// suspended at a yield inside F escapes without re-running F.
    ///
    /// D3a adds the third copy: a `return` inside B (or a nested
    /// catch body) stores its value and gotos `fin_ret_entry`, whose
    /// F copy ends by RE-LOWERING a `return this.<slot>` — which
    /// consults the OUTER frames (this frame is popped by then), so
    /// nested finally chains run outside-in for free.
    pub(super) fn lower_try_finally(&mut self, body: Vec<Stmt>, f: Vec<Stmt>) {
        // D3b — a yield in B (or a nested C) puts a suspendable state
        // inside the region, making it a `return()` injection target:
        // the return copy below must then exist even when B routes no
        // return of its own. Yield-free bodies can't suspend in range,
        // so injection never matches them and the copy stays unminted.
        let body_has_yield = body.iter().any(stmt_contains_yield);
        let try_entry = self.alloc_state();
        let mut g = self.emit_goto(try_entry);
        self.cur_buf.append(&mut g);
        self.flush_cur();

        self.cur_state = try_entry;
        self.finally_ret.push(FinallyRetFrame {
            try_entry,
            slot: format!("__retval{try_entry}"),
            gotos: Vec::new(),
            jump_gotos: Vec::new(),
        });
        self.lower_seq(body);
        let frame = self.finally_ret.pop().expect("pushed above");
        let region_end = self.arms.len() - 1;
        // Normal copy gets its OWN entry state: B's tail otherwise
        // shares its arm with F's first chunk, and with an empty B
        // that arm IS the region — an exception inside F would
        // wrongly route back to the exception copy and run F twice.
        let fin_norm_entry = self.alloc_state();
        let mut to_norm = self.emit_goto(fin_norm_entry);
        self.cur_buf.append(&mut to_norm);
        self.flush_cur();

        self.cur_state = fin_norm_entry;
        self.lower_seq(f.clone());
        let fin_exc_entry = self.alloc_state();
        let post = self.alloc_state();
        let mut exit = self.emit_goto(post);
        self.cur_buf.append(&mut exit);
        self.flush_cur();

        self.cur_state = fin_exc_entry;
        let slot = format!("__caught{fin_exc_entry}");
        self.hoisted.push((slot.clone(), "any".into()));
        self.lower_seq(f.clone());
        let this_id = self.ast.add_expr(Expr::This);
        let slot_read = self.ast.add_expr(Expr::Member {
            obj: this_id,
            name: slot.clone(),
        });
        self.cur_buf.push(Stmt::Throw(slot_read));
        self.flush_cur();

        // D4 escaping-jump copies — one per (kind, label) B routed.
        self.mint_jump_copies(&f, frame.jump_gotos);

        // D3a return copy — when B actually routed a return, or (D3b)
        // when B can suspend in range so a `return()` injection could
        // route here.
        let mut ret = None;
        if !frame.gotos.is_empty() || body_has_yield {
            let fin_ret_entry = self.alloc_state();
            self.hoisted.push((frame.slot.clone(), "any".into()));
            self.cur_state = fin_ret_entry;
            self.lower_seq(f);
            let this_id = self.ast.add_expr(Expr::This);
            let ret_read = self.ast.add_expr(Expr::Member {
                obj: this_id,
                name: frame.slot.clone(),
            });
            // Typed lane unboxes the any slot back to the step's
            // value type; the any lane's As-wrap happens inside
            // make_done_step on the plain read.
            let ret_val = if self.yield_ty == "any" {
                ret_read
            } else {
                let ty = self.yield_ty.clone();
                self.ast.add_expr(Expr::As {
                    expr: ret_read,
                    ty_ann: ty,
                })
            };
            // Re-LOWER the return so any outer finally frame chains.
            self.lower(Stmt::Return(Some(ret_val)));
            self.flush_cur();
            for eid in frame.gotos {
                self.ast.exprs[eid.0 as usize] = Expr::Number(fin_ret_entry as f64);
            }
            ret = Some((fin_ret_entry, frame.slot));
        }

        self.cur_state = post;
        self.regions.push(TryRegion {
            start: try_entry,
            end: region_end,
            catch_entry: fin_exc_entry,
            slot,
            ret,
        });
    }
}

/// D1 gate — true when `stmts` contain something the finally-region
/// lowering can't route: a bare jump on a switch surface (a bare
/// `break` there is switch-owned, not loop-owned — distinguishing
/// them isn't worth the precision, fallback keeps today's shape), or
/// a labeled jump whose target is neither declared inside the try
/// nor an enclosing yield-loop's label (`outer_labels`) — e.g. a
/// labeled BLOCK outside the try, which the SM resolver can't reach
/// (labeled blocks never enter the loop stack), so the jump would
/// survive as a literal naming a label the state-machining erased.
/// Returns route at ANY depth (the D3a goto is a `continue __sm;`,
/// [`DISPATCH_LABEL`]); escaping jumps — bare or labeled — route
/// through the per-(kind, label) D4 finally copies.
pub(super) fn stmts_block_finally_region(stmts: &[Stmt], outer_labels: &[String]) -> bool {
    fn collect_labels(s: &Stmt, out: &mut Vec<String>) {
        match s {
            Stmt::Labeled { label, body } => {
                out.push(label.clone());
                collect_labels(body, out);
            }
            Stmt::Block(ss) | Stmt::Multi(ss) => ss.iter().for_each(|x| collect_labels(x, out)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_labels(then_branch, out);
                if let Some(e) = else_branch.as_deref() {
                    collect_labels(e, out);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. }
            | Stmt::For { body, .. } => collect_labels(body, out),
            Stmt::Switch { cases, default, .. } => {
                cases
                    .iter()
                    .for_each(|c| c.body.iter().for_each(|x| collect_labels(x, out)));
                if let Some(ds) = default {
                    ds.iter().for_each(|x| collect_labels(x, out));
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => body
                .iter()
                .chain(catch_body.iter())
                .chain(finally_body.iter().flatten())
                .for_each(|x| collect_labels(x, out)),
            _ => {}
        }
    }
    fn walk(s: &Stmt, known: &[String], in_switch: bool) -> bool {
        match s {
            Stmt::Break(l) | Stmt::Continue(l) => match l {
                Some(name) => !known.contains(name),
                None => in_switch,
            },
            Stmt::Block(ss) | Stmt::Multi(ss) => ss.iter().any(|x| walk(x, known, in_switch)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(then_branch, known, in_switch)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| walk(e, known, in_switch))
            }
            Stmt::Labeled { body, .. } => walk(body, known, in_switch),
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => walk(body, known, false),
            Stmt::For { init, body, .. } => {
                init.as_deref().is_some_and(|i| walk(i, known, in_switch))
                    || walk(body, known, false)
            }
            Stmt::Switch { cases, default, .. } => {
                cases
                    .iter()
                    .any(|c| c.body.iter().any(|x| walk(x, known, true)))
                    || default
                        .as_ref()
                        .is_some_and(|d| d.iter().any(|x| walk(x, known, true)))
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => body
                .iter()
                .chain(catch_body.iter())
                .chain(finally_body.iter().flatten())
                .any(|x| walk(x, known, in_switch)),
            _ => false,
        }
    }
    let mut known: Vec<String> = outer_labels.to_vec();
    for s in stmts {
        collect_labels(s, &mut known);
    }
    stmts.iter().any(|s| walk(s, &known, false))
}
