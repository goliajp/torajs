//! Yield-bearing `while` / `for` arms of the generator state-machine
//! emitter — verbatim move out of `desugar_generators_sm` when the
//! RFC 20260802 upgrade blade (labeled dispatch loop + outer-jump
//! rewrite hooks) pushed that file past the 500-line HARD limit.
//! Both methods are consumed only by [`GenSm::lower`].

use super::desugar_generators_sm::GenSm;
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{ExprId, Stmt};

impl GenSm<'_> {
    /// Yield-bearing `while (cond) { body }`: head state re-checks the
    /// condition each turn, body state lowers with the loop's
    /// (continue → head, break → post) targets on the stack.
    pub(super) fn lower_while(&mut self, cond: ExprId, body: Box<Stmt>) {
        if !stmt_contains_yield(&body) {
            let mut s = Stmt::While { cond, body };
            self.rewrite_outer_jumps(&mut s);
            self.rewrite_nested_returns(&mut s);
            self.cur_buf.push(s);
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
        let label = self.pending_label.take();
        self.loop_stack.push((head, post, label));
        self.lower(*body);
        self.loop_stack.pop();
        let mut back = self.emit_goto(head);
        self.cur_buf.append(&mut back);
        self.flush_cur();

        self.cur_state = post;
    }

    /// Yield-bearing `for (init; cond; step) { body }`: init lowers
    /// into the current state, then head / body / step / post states
    /// mirror the while shape with continue targeting the step state.
    pub(super) fn lower_for(
        &mut self,
        init: Option<Box<Stmt>>,
        cond: Option<ExprId>,
        step: Option<ExprId>,
        body: Box<Stmt>,
    ) {
        if !stmt_contains_yield(&body) && !init.as_deref().is_some_and(stmt_contains_yield) {
            let mut s = Stmt::For {
                init,
                cond,
                step,
                body,
            };
            self.rewrite_outer_jumps(&mut s);
            self.rewrite_nested_returns(&mut s);
            self.cur_buf.push(s);
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
        let label = self.pending_label.take();
        self.loop_stack.push((step_state, post, label));
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
}
