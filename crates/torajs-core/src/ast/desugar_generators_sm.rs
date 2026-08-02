//! Generator state-machine emitter.
//!
//! Chunk 341 — extracted from `ast::desugar_generators` so the entire
//! state-machine core (yield-detection helper, break/continue
//! rewriter, GenSm struct + impl) lives next to the other
//! generator-desugar siblings. The caller-side
//! `build_state_machine_next_body` (in ast.rs) still owns the final
//! while-true assembly + tail-return because it threads the result
//! into the ClassMethod; here we expose just GenSm and its arms.
//!
//! Visibility surface to ast.rs:
//! - `GenSm` (struct) — `pub(super)`
//! - fields `ast` / `arms` / `cur_buf` — `pub(super)` (caller reads
//!   arms after lower_seq, pushes tail return into cur_buf, and
//!   reads ast for follow-up Expr alloc)
//! - methods `new` / `lower_seq` / `flush_cur` — `pub(super)`
//! - All other items (alloc_state, emit_*, lower, the two helpers)
//!   stay file-private to this sibling.

use super::desugar_generators_sm_rewrite::{rewrite_break_continue_for_outer, stmt_contains_yield};
use super::{Ast, Expr, ExprId, Stmt};

/// The local `next()` dispatches on, seeded from `this.__state` at
/// entry — which is set to the dead sentinel in the same breath.
///
/// The persisted field therefore says "resumable HERE" only while the
/// generator is suspended at a yield, and a `next()` that leaves any
/// other way leaves it dead. That is what ES §27.5.1.2 requires of an
/// abrupt completion: a generator whose body throws is COMPLETED, and
/// a later `next()` answers `{ value: undefined, done: true }` rather
/// than re-entering the body. Persisting the label on every internal
/// goto instead (the pre-fix shape) left the field pointing at the arm
/// the throw escaped from, so the next call re-ran it and threw the
/// same error again — test262's `iter-step-err` family asserts exactly
/// this ("Iterator is closed following abrupt completion").
pub(super) const RESUME_LOCAL: &str = "__gen_st";

/// Label on the dispatch `while (true)` loop. Every goto is a
/// `continue __sm;` so it re-enters the dispatch from ANY nesting
/// depth — a bare `continue` inside an inline-emitted (yield-free)
/// inner loop would bind to that loop instead, which is why the
/// finally gate walker used to reject returns inside inner loops.
/// The labeled form is also immune to reinterpretation: neither the
/// SM Continue arm nor the outer-jump rewriter can resolve `__sm`
/// (it never names a yield-loop on the stack), so a routed goto
/// passes through both untouched. `__`-prefixed like every other
/// desugar-reserved name.
pub(super) const DISPATCH_LABEL: &str = "__sm";

/// State-machine emitter for generator bodies. Each state's body is
/// accumulated into `cur_buf` and flushed into `arms[cur_state]` when
/// the state ends (via yield, goto, or descent into a nested state).
///
/// The final assembled if-chain is wrapped in `while (true) { ... }`
/// so `Stmt::Continue` can be used as the goto primitive — setting
/// `this.__state = N; continue;` re-enters the chain at state N.
pub(super) struct GenSm<'a> {
    pub(super) ast: &'a mut Ast,
    pub(super) arms: Vec<Vec<Stmt>>,
    pub(super) cur_state: usize,
    pub(super) cur_buf: Vec<Stmt>,
    /// RFC 20260802 — exception regions recorded by the `Stmt::Try`
    /// arm (sibling `desugar_generators_sm_try`). Non-empty ⇒ the
    /// assembly wraps the dispatch if-chain in a try/catch that
    /// routes a throw from any state in `[start, end]` to the
    /// region's catch-entry state (regenerator tryEntries shape).
    pub(super) regions: Vec<super::desugar_generators_sm_try::TryRegion>,
    /// Extra `this.<name>` fields minted during SM lowering (the
    /// hoisted catch-param slots). The driver appends these to
    /// `lifted_locals` so they become class fields.
    pub(super) hoisted: Vec<(String, String)>,
    /// D3a — enclosing try/finally frames (innermost last). A
    /// `return v` inside such a frame's try body must run F on the
    /// way out: it stores v into the frame's slot and gotos the
    /// frame's return-copy entry — whose state number isn't known
    /// until the body finishes lowering, so the goto literal is a
    /// placeholder ExprId patched by `lower_try_finally` afterwards.
    pub(super) finally_ret: Vec<super::desugar_generators_sm_finally::FinallyRetFrame>,
    /// (continue_target, break_target, label) for each enclosing
    /// yield-loop. Yield-FREE inner loops emit inline — their
    /// break/continue keep their normal Stmt::Break / Stmt::Continue
    /// meaning, never enter this stack. `label` is `Some` when the
    /// yield-loop was wrapped in a `Stmt::Labeled`, so `break label` /
    /// `continue label` naming it resolve to its state (ES §13.13).
    pub(super) loop_stack: Vec<(usize, usize, Option<String>)>,
    /// Set by the `Stmt::Labeled` arm just before lowering a
    /// yield-bearing labeled loop; the loop's arm `take`s it into its
    /// `loop_stack` entry. `None` outside that hand-off.
    pub(super) pending_label: Option<String>,
    /// P10.7 — the generator's yield type ann. When `"any"`,
    /// `emit_yield_return` wraps the yielded value in `Expr::As { ...,
    /// ty_ann: "any" }` so the step's `value` field write goes through
    /// the existing box-to-Any machinery (lowered as NaN-box AnyValue).
    /// Without the wrap the field write hits a layout mismatch (step
    /// declares `value: any` but the ObjectLit's field_tys carries the
    /// concrete primitive type) and SIGSEGVs.
    pub(super) yield_ty: String,
}

impl<'a> GenSm<'a> {
    pub(super) fn new(ast: &'a mut Ast, yield_ty: String) -> Self {
        Self {
            ast,
            arms: vec![Vec::new()],
            cur_state: 0,
            cur_buf: Vec::new(),
            regions: Vec::new(),
            hoisted: Vec::new(),
            finally_ret: Vec::new(),
            loop_stack: Vec::new(),
            pending_label: None,
            yield_ty,
        }
    }

    pub(super) fn alloc_state(&mut self) -> usize {
        let s = self.arms.len();
        self.arms.push(Vec::new());
        s
    }

    /// State to `goto` for a `break` / `continue` (`want_break` picks
    /// break vs continue target). `None` label → innermost yield-loop;
    /// `Some(l)` → the enclosing yield-loop labeled `l` (ES §13.13).
    /// Returns `None` when no matching yield-loop is on the stack — the
    /// jump then stays literal (it belongs to a yield-free inner loop
    /// or is resolved later by ssa_lower).
    fn sm_loop_target(&self, label: &Option<String>, want_break: bool) -> Option<usize> {
        let pick = |&(cont, brk, _): &(usize, usize, Option<String>)| {
            if want_break { brk } else { cont }
        };
        match label {
            None => self.loop_stack.last().map(pick),
            Some(l) => self
                .loop_stack
                .iter()
                .rev()
                .find(|(_, _, lbl)| lbl.as_deref() == Some(l.as_str()))
                .map(pick),
        }
    }

    /// Labels of the enclosing yield-bearing loops — the finally
    /// gate walker's outer-label set (a labeled jump resolving to
    /// one of these routes through the SM, so it doesn't block a
    /// finally region).
    pub(super) fn outer_loop_labels(&self) -> Vec<String> {
        self.loop_stack
            .iter()
            .filter_map(|(_, _, l)| l.clone())
            .collect()
    }

    pub(super) fn flush_cur(&mut self) {
        let cur = self.cur_state;
        let buf = std::mem::take(&mut self.cur_buf);
        self.arms[cur].extend(buf);
    }

    /// Persist the resume label into `this.__state`. Only a YIELD does
    /// this: it is the one exit that leaves the generator resumable.
    /// See [`RESUME_LOCAL`] for why nothing else may write the field.
    fn emit_set_state(&mut self, target: usize) -> Stmt {
        let this_id = self.ast.add_expr(Expr::This);
        let m = self.ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__state".into(),
        });
        let lit = self.ast.add_expr(Expr::Number(target as f64));
        let assign = self.ast.add_expr(Expr::Assign {
            target: m,
            value: lit,
        });
        Stmt::Expr(assign)
    }

    /// A goto is a transition WITHIN one `next()` call, so it moves the
    /// local resume cursor, not the persisted field — see
    /// [`RESUME_LOCAL`]. The continue names the dispatch loop's label
    /// so it reaches it from any nesting depth ([`DISPATCH_LABEL`]).
    pub(super) fn emit_goto(&mut self, target: usize) -> Vec<Stmt> {
        let st = self.ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let lit = self.ast.add_expr(Expr::Number(target as f64));
        let assign = self.ast.add_expr(Expr::Assign {
            target: st,
            value: lit,
        });
        vec![
            Stmt::Expr(assign),
            Stmt::Continue(Some(DISPATCH_LABEL.into())),
        ]
    }

    fn emit_yield_return(&mut self, val: ExprId, next: usize) -> Vec<Stmt> {
        let set = self.emit_set_state(next);
        // P10.7 — Default-Any yield: route through `Expr::As { …,
        // ty_ann: "any" }` so the step's `value: any` field write
        // picks up the existing box-to-Any path (NaN-box AnyValue).
        // Explicit-T generators (`yield_ty != "any"`) keep their
        // direct write — both branches still produce the same
        // step shape from the user's perspective.
        let val_for_step = if self.yield_ty == "any" {
            self.ast.add_expr(Expr::As {
                expr: val,
                ty_ann: "any".into(),
            })
        } else {
            val
        };
        let done = self.ast.add_expr(Expr::Bool(false));
        let obj = self.ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val_for_step), ("done".into(), done)],
        });
        vec![set, Stmt::Return(Some(obj))]
    }

    pub(super) fn lower_seq(&mut self, stmts: Vec<Stmt>) {
        for s in stmts {
            self.lower(s);
        }
    }

    pub(super) fn lower(&mut self, stmt: Stmt) {
        match stmt {
            // RFC 20260802 — try/catch with yield lowers into an
            // exception region (sibling `desugar_generators_sm_try`);
            // yield-free / finally-bearing trys fall back to the
            // verbatim inline push inside `lower_try`.
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => self.lower_try(
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            ),
            Stmt::Yield(e) => {
                let next = self.alloc_state();
                let mut yr = self.emit_yield_return(e, next);
                self.cur_buf.append(&mut yr);
                self.flush_cur();
                self.cur_state = next;
            }
            // S2.30 — a user `return v;` in the generator body: the
            // value becomes the DONE step's `value` and the generator
            // completes (§27.5.1.2 GeneratorResume step 10 / return
            // completion). Pre-arm the bare return leaked into the
            // `next()` body verbatim and hit the step-struct return
            // type check ("return type mismatch: expects Struct([value,
            // done]), got Number"). No state store is needed — entry
            // already stamped the dead sentinel (RESUME_LOCAL doc), so
            // any exit that isn't a yield leaves the generator
            // completed. Value routing mirrors `emit_yield_return`
            // (Default-Any generators box via `As any`).
            Stmt::Return(v) => {
                // D3a — inside a try/finally frame the return routes
                // through F's return copy instead of completing here.
                if !self.finally_ret.is_empty() {
                    self.emit_return_through_finally(v);
                    return;
                }
                let obj = self.make_done_step(v);
                self.cur_buf.push(Stmt::Return(Some(obj)));
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                for s in stmts {
                    self.lower(s);
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_has = stmt_contains_yield(&then_branch);
                let else_has = else_branch.as_deref().is_some_and(stmt_contains_yield);
                if !then_has && !else_has {
                    let mut s = Stmt::If {
                        cond,
                        then_branch,
                        else_branch,
                    };
                    self.rewrite_outer_jumps(&mut s);
                    self.rewrite_nested_returns(&mut s);
                    self.cur_buf.push(s);
                    return;
                }
                let then_entry = self.alloc_state();
                let post = self.alloc_state();
                let else_entry = if else_branch.is_some() {
                    self.alloc_state()
                } else {
                    post
                };
                let then_jump = self.emit_goto(then_entry);
                let else_jump = self.emit_goto(else_entry);
                self.cur_buf.push(Stmt::If {
                    cond,
                    then_branch: Box::new(Stmt::Block(then_jump)),
                    else_branch: Some(Box::new(Stmt::Block(else_jump))),
                });
                self.flush_cur();

                self.cur_state = then_entry;
                self.lower(*then_branch);
                let mut exit = self.emit_goto(post);
                self.cur_buf.append(&mut exit);
                self.flush_cur();

                if let Some(eb) = else_branch {
                    self.cur_state = else_entry;
                    self.lower(*eb);
                    let mut exit = self.emit_goto(post);
                    self.cur_buf.append(&mut exit);
                    self.flush_cur();
                }

                self.cur_state = post;
            }
            Stmt::While { cond, body } => self.lower_while(cond, body),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => self.lower_for(init, cond, step, body),
            Stmt::Continue(label) => {
                if let Some(cont) = self.sm_loop_target(&label, false) {
                    // D4 — a continue (bare or labeled) whose loop
                    // sits outside an enclosing try/finally runs F
                    // first via a per-(kind, label) F copy.
                    let mut g = if self.jump_escapes_finally(cont) {
                        self.build_finally_jump_stmts(false, label)
                    } else {
                        self.emit_goto(cont)
                    };
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Continue(label));
                }
            }
            Stmt::Break(label) => {
                if let Some(brk) = self.sm_loop_target(&label, true) {
                    let mut g = if self.jump_escapes_finally(brk) {
                        self.build_finally_jump_stmts(true, label)
                    } else {
                        self.emit_goto(brk)
                    };
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Break(label));
                }
            }
            Stmt::Labeled { label, body } => self.lower_labeled(label, body),
            mut other => {
                self.rewrite_outer_jumps(&mut other);
                self.rewrite_nested_returns(&mut other);
                self.cur_buf.push(other);
            }
        }
    }

    /// Run the outer-jump rewriter over an inline-emitted stmt when
    /// any yield-loop is live — a labeled jump inside it (even under
    /// an inner loop, switch, or verbatim try) that names an
    /// enclosing yield-loop must become a goto, since the loop it
    /// targets is state-machined away.
    pub(super) fn rewrite_outer_jumps(&mut self, s: &mut Stmt) {
        if !self.loop_stack.is_empty() {
            let stack = self.loop_stack.clone();
            rewrite_break_continue_for_outer(self.ast, s, &stack, &mut self.finally_ret);
        }
    }

    /// `label: stmt` inside a generator body — ES §13.13. A yield-bearing
    /// labeled loop hands its label to the loop's arm (recorded on
    /// `loop_stack` so `break label` / `continue label` naming it resolve
    /// to its state); a yield-free labeled statement is emitted inline
    /// with its label preserved so ssa_lower resolves the jumps, while
    /// bare / this-loop labeled jumps targeting an enclosing SM loop still
    /// rewrite to gotos.
    fn lower_labeled(&mut self, label: String, body: Box<Stmt>) {
        if stmt_contains_yield(&body) {
            self.pending_label = Some(label);
            self.lower(*body);
            self.pending_label = None;
        } else {
            let mut s = Stmt::Labeled { label, body };
            self.rewrite_outer_jumps(&mut s);
            self.cur_buf.push(s);
        }
    }
}
