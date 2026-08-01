//! Generator try/finally lowering — RFC 20260802 D1 + D3a.
//!
//! Split out of `desugar_generators_sm_try` when the D3a return
//! routing pushed that file past the 500-line HARD limit. Owns the
//! per-exit-path finally duplication (`lower_try_finally`), the
//! return-through-finally routing (frame stack + placeholder-patched
//! gotos), and the conservative gate walker. The catch-region arm,
//! dispatch wrap, and throw-injection stay in the `_sm_try` sibling.

use super::desugar_generators_sm::{GenSm, RESUME_LOCAL};
use super::desugar_generators_sm_try::TryRegion;
use super::{Expr, Stmt};

/// D3a — one enclosing try/finally frame. A `return v` under it
/// stores v into `this.<slot>` and gotos F's return copy; the copy's
/// state number is unknown until the body finishes lowering, so each
/// goto's Number literal is recorded here and patched afterwards.
pub(super) struct FinallyRetFrame {
    pub(super) slot: String,
    pub(super) gotos: Vec<super::ExprId>,
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
    /// __gen_st = <placeholder>; continue;]`. The placeholder literal
    /// is recorded on the innermost frame for the patch. Inline
    /// callers are continue-safe by construction — the finally gate
    /// walker falls back on any return sitting inside an inner loop,
    /// where a bare `continue` would bind wrong.
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
            Stmt::Continue(None),
        ]
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
        let try_entry = self.alloc_state();
        let mut g = self.emit_goto(try_entry);
        self.cur_buf.append(&mut g);
        self.flush_cur();

        self.cur_state = try_entry;
        self.finally_ret.push(FinallyRetFrame {
            slot: format!("__retval{try_entry}"),
            gotos: Vec::new(),
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

        // D3a return copy — only when B actually routed a return.
        if !frame.gotos.is_empty() {
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
        }

        self.cur_state = post;
        self.regions.push(TryRegion {
            start: try_entry,
            end: region_end,
            catch_entry: fin_exc_entry,
            slot,
        });
    }
}

/// D1 gate — true when `stmts` contain something the finally-region
/// lowering can't route: a labeled jump (could escape the try at any
/// depth), a bare break/continue not owned by an inner loop, or —
/// since the D3a return routing rewrites a return into a bare
/// `continue` back to the dispatch — a `return` sitting inside an
/// inner loop, where that continue would bind wrong. Returns at
/// continue-safe depths route through the finally copies (D3a);
/// returns IN the finally body itself (`allow_return`) legitimately
/// override the completion. Conservative in the fallback direction:
/// switch bodies count as escaping surface for bare jumps too (a
/// bare `continue` inside switch escapes to the loop; distinguishing
/// it from switch-owned `break` isn't worth the precision — fallback
/// just keeps today's shape).
pub(super) fn stmts_block_finally_region(stmts: &[Stmt], allow_return: bool) -> bool {
    fn walk(s: &Stmt, allow_return: bool, in_loop: bool) -> bool {
        match s {
            Stmt::Return(_) => !allow_return && in_loop,
            Stmt::Break(l) | Stmt::Continue(l) => l.is_some() || !in_loop,
            Stmt::Block(ss) | Stmt::Multi(ss) => ss.iter().any(|x| walk(x, allow_return, in_loop)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                walk(then_branch, allow_return, in_loop)
                    || else_branch
                        .as_deref()
                        .is_some_and(|e| walk(e, allow_return, in_loop))
            }
            Stmt::Labeled { body, .. } => walk(body, allow_return, in_loop),
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::ForOf { body, .. }
            | Stmt::ForOfSplitIter { body, .. } => walk(body, allow_return, true),
            Stmt::For { init, body, .. } => {
                init.as_deref()
                    .is_some_and(|i| walk(i, allow_return, in_loop))
                    || walk(body, allow_return, true)
            }
            Stmt::Switch { cases, default, .. } => {
                cases
                    .iter()
                    .any(|c| c.body.iter().any(|x| walk(x, allow_return, in_loop)))
                    || default
                        .as_ref()
                        .is_some_and(|d| d.iter().any(|x| walk(x, allow_return, in_loop)))
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
                .any(|x| walk(x, allow_return, in_loop)),
            _ => false,
        }
    }
    stmts.iter().any(|s| walk(s, allow_return, false))
}
