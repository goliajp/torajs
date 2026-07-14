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

/// Returns true if `s` (or any nested stmt) contains a `yield`. Used
/// by `GenSm` to decide whether a control-flow construct must be
/// expanded into separate state arms (yields present) or can be
/// emitted inline as a regular Stmt::If / While / For.
fn stmt_contains_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield(_) | Stmt::YieldInto { .. } => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_contains_yield(then_branch)
                || else_branch.as_deref().is_some_and(stmt_contains_yield)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_contains_yield(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_yield) || stmt_contains_yield(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(stmt_contains_yield),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| c.body.iter().any(stmt_contains_yield))
                || default
                    .as_ref()
                    .is_some_and(|d| d.iter().any(stmt_contains_yield))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(stmt_contains_yield)
                || catch_body.iter().any(stmt_contains_yield)
                || finally_body
                    .as_ref()
                    .is_some_and(|f| f.iter().any(stmt_contains_yield))
        }
        _ => false,
    }
}

/// Rewrite `continue;` / `break;` inside `s` into `state = <target>;
/// continue;` gotos that re-enter the enclosing `while (true)` state
/// machine at the loop's continue / break target. Stops at inner loop
/// boundaries — break/continue inside a nested yield-free
/// `while` / `for` belong to that inner loop and stay literal.
fn rewrite_break_continue_for_outer(
    ast: &mut Ast,
    s: &mut Stmt,
    cont_target: usize,
    brk_target: usize,
) {
    /// A rewritten `break` / `continue` is a goto, so it moves the local
    /// resume cursor — same as [`GenSm::emit_goto`], and for the same
    /// reason (see [`RESUME_LOCAL`]). Writing `this.__state` here while
    /// the dispatch reads the local would re-enter the SAME arm on every
    /// turn of the `while (true)`: an infinite loop in any generator
    /// whose yield-bearing loop breaks or continues.
    fn make_goto(ast: &mut Ast, target: usize) -> Stmt {
        let st = ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let lit = ast.add_expr(Expr::Number(target as f64));
        let assign = ast.add_expr(Expr::Assign {
            target: st,
            value: lit,
        });
        Stmt::Block(vec![Stmt::Expr(assign), Stmt::Continue])
    }
    match s {
        Stmt::Continue => *s = make_goto(ast, cont_target),
        Stmt::Break => *s = make_goto(ast, brk_target),
        // Inner loops own their break/continue — don't descend.
        Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
        // Switch swallows `break` (it targets the switch). `continue`
        // inside a switch belongs to the enclosing loop, but yields
        // inside switch aren't in J.2.b scope so we don't touch this.
        Stmt::Switch { .. } => {}
        Stmt::Try { .. } => {}
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_break_continue_for_outer(ast, then_branch, cont_target, brk_target);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_break_continue_for_outer(ast, eb, cont_target, brk_target);
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_break_continue_for_outer(ast, s, cont_target, brk_target);
            }
        }
        _ => {}
    }
}

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
    cur_state: usize,
    pub(super) cur_buf: Vec<Stmt>,
    /// (continue_target, break_target) for each enclosing yield-loop.
    /// Yield-FREE inner loops emit inline — their break/continue keep
    /// their normal Stmt::Break / Stmt::Continue meaning, never enter
    /// this stack.
    loop_stack: Vec<(usize, usize)>,
    /// P10.7 — the generator's yield type ann. When `"any"`,
    /// `emit_yield_return` wraps the yielded value in `Expr::As { ...,
    /// ty_ann: "any" }` so the step's `value` field write goes through
    /// the existing box-to-Any machinery (lowered as NaN-box AnyValue).
    /// Without the wrap the field write hits a layout mismatch (step
    /// declares `value: any` but the ObjectLit's field_tys carries the
    /// concrete primitive type) and SIGSEGVs.
    yield_ty: String,
}

impl<'a> GenSm<'a> {
    pub(super) fn new(ast: &'a mut Ast, yield_ty: String) -> Self {
        Self {
            ast,
            arms: vec![Vec::new()],
            cur_state: 0,
            cur_buf: Vec::new(),
            loop_stack: Vec::new(),
            yield_ty,
        }
    }

    fn alloc_state(&mut self) -> usize {
        let s = self.arms.len();
        self.arms.push(Vec::new());
        s
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
    /// [`RESUME_LOCAL`].
    fn emit_goto(&mut self, target: usize) -> Vec<Stmt> {
        let st = self.ast.add_expr(Expr::Ident(RESUME_LOCAL.into()));
        let lit = self.ast.add_expr(Expr::Number(target as f64));
        let assign = self.ast.add_expr(Expr::Assign {
            target: st,
            value: lit,
        });
        vec![Stmt::Expr(assign), Stmt::Continue]
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

    fn lower(&mut self, stmt: Stmt) {
        match stmt {
            Stmt::Yield(e) => {
                let next = self.alloc_state();
                let mut yr = self.emit_yield_return(e, next);
                self.cur_buf.append(&mut yr);
                self.flush_cur();
                self.cur_state = next;
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
                    if let Some(&(cont, brk)) = self.loop_stack.last() {
                        rewrite_break_continue_for_outer(self.ast, &mut s, cont, brk);
                    }
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
            Stmt::While { cond, body } => {
                if !stmt_contains_yield(&body) {
                    self.cur_buf.push(Stmt::While { cond, body });
                    return;
                }
                let head = self.alloc_state();
                let body_entry = self.alloc_state();
                let post = self.alloc_state();

                let mut to_head = self.emit_goto(head);
                self.cur_buf.append(&mut to_head);
                self.flush_cur();

                self.cur_state = head;
                let then_jump = self.emit_goto(body_entry);
                let else_jump = self.emit_goto(post);
                self.cur_buf.push(Stmt::If {
                    cond,
                    then_branch: Box::new(Stmt::Block(then_jump)),
                    else_branch: Some(Box::new(Stmt::Block(else_jump))),
                });
                self.flush_cur();

                self.cur_state = body_entry;
                self.loop_stack.push((head, post));
                self.lower(*body);
                self.loop_stack.pop();
                let mut back = self.emit_goto(head);
                self.cur_buf.append(&mut back);
                self.flush_cur();

                self.cur_state = post;
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if !stmt_contains_yield(&body) && !init.as_deref().is_some_and(stmt_contains_yield)
                {
                    self.cur_buf.push(Stmt::For {
                        init,
                        cond,
                        step,
                        body,
                    });
                    return;
                }
                if let Some(i) = init {
                    self.lower(*i);
                }
                let head = self.alloc_state();
                let body_entry = self.alloc_state();
                let step_state = self.alloc_state();
                let post = self.alloc_state();

                let mut to_head = self.emit_goto(head);
                self.cur_buf.append(&mut to_head);
                self.flush_cur();

                self.cur_state = head;
                if let Some(c) = cond {
                    let then_jump = self.emit_goto(body_entry);
                    let else_jump = self.emit_goto(post);
                    self.cur_buf.push(Stmt::If {
                        cond: c,
                        then_branch: Box::new(Stmt::Block(then_jump)),
                        else_branch: Some(Box::new(Stmt::Block(else_jump))),
                    });
                } else {
                    let mut g = self.emit_goto(body_entry);
                    self.cur_buf.append(&mut g);
                }
                self.flush_cur();

                self.cur_state = body_entry;
                self.loop_stack.push((step_state, post));
                self.lower(*body);
                self.loop_stack.pop();
                let mut to_step = self.emit_goto(step_state);
                self.cur_buf.append(&mut to_step);
                self.flush_cur();

                self.cur_state = step_state;
                if let Some(s) = step {
                    self.cur_buf.push(Stmt::Expr(s));
                }
                let mut back = self.emit_goto(head);
                self.cur_buf.append(&mut back);
                self.flush_cur();

                self.cur_state = post;
            }
            Stmt::Continue => {
                if let Some(&(cont, _)) = self.loop_stack.last() {
                    let mut g = self.emit_goto(cont);
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Continue);
                }
            }
            Stmt::Break => {
                if let Some(&(_, brk)) = self.loop_stack.last() {
                    let mut g = self.emit_goto(brk);
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Break);
                }
            }
            other => self.cur_buf.push(other),
        }
    }
}
