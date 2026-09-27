//! Yield-bearing branch arms of the generator state-machine emitter,
//! moved out of `desugar_generators_sm` for file size. Consumed only
//! by [`GenSm::lower`].

use super::desugar_generators_sm::GenSm;
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{ExprId, Stmt};

impl GenSm<'_> {
    /// `if (cond) then else`: inline when neither branch yields;
    /// otherwise each branch gets its own entry state and both rejoin
    /// at a post state.
    pub(super) fn lower_if(
        &mut self,
        cond: ExprId,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    ) {
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
}
