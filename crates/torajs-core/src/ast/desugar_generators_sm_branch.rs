//! Yield-bearing branch arms (if, switch) of the generator state-machine emitter,
//! moved out of `desugar_generators_sm` for file size. Consumed only
//! by [`GenSm::lower`].

use super::desugar_generators_sm::GenSm;
use super::desugar_generators_sm_jump::SmJump;
use super::desugar_generators_sm_rewrite::stmt_contains_yield;
use super::{ExprId, Stmt, SwitchCase};

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

    /// `switch (scrutinee) { … }` holding a yield. The dispatch stays
    /// one inline switch whose clauses only goto their body's state,
    /// so the scrutinee evaluates once, the case expressions evaluate
    /// in order and only until one matches, and `default` is taken
    /// only when none does (§14.12.4 CaseBlockEvaluation) — the plain
    /// switch lowering already does all of that. Each clause body gets
    /// its own state in source order and falls through by a goto to
    /// the next; a bare `break` exits to the post state, and a bare
    /// `continue` passes the switch by for the enclosing loop.
    pub(super) fn lower_switch(
        &mut self,
        scrutinee: ExprId,
        cases: Vec<SwitchCase>,
        default: Option<Vec<Stmt>>,
        default_at: usize,
    ) {
        let has_yield = cases
            .iter()
            .flat_map(|c| c.body.iter())
            .chain(default.iter().flatten())
            .any(stmt_contains_yield);
        if !has_yield {
            let mut s = Stmt::Switch {
                scrutinee,
                cases,
                default,
                default_at,
            };
            self.rewrite_outer_jumps(&mut s);
            self.rewrite_nested_returns(&mut s);
            self.cur_buf.push(s);
            return;
        }
        // The post state is allocated BEFORE the clause states, so a
        // `break` from inside a try/finally a clause holds sees it as
        // escaping (`jump_escapes_finally`) and routes through F.
        let post = self.alloc_state();
        let mut dispatch: Vec<SwitchCase> = Vec::new();
        let mut bodies: Vec<(usize, Vec<Stmt>)> = Vec::new();
        for c in cases {
            let entry = self.alloc_state();
            dispatch.push(SwitchCase {
                value: c.value,
                body: self.emit_goto(entry),
            });
            bodies.push((entry, c.body));
        }
        let default = default.map(|body| (self.alloc_state(), body));
        let dispatch_default = default.as_ref().map(|(entry, _)| self.emit_goto(*entry));
        self.cur_buf.push(Stmt::Switch {
            scrutinee,
            cases: dispatch,
            default: dispatch_default,
            default_at,
        });
        let mut no_match = self.emit_goto(post);
        self.cur_buf.append(&mut no_match);
        self.flush_cur();

        let after = bodies.split_off(default_at);
        bodies.extend(default);
        bodies.extend(after);
        let next: Vec<usize> = bodies
            .iter()
            .skip(1)
            .map(|(e, _)| *e)
            .chain([post])
            .collect();
        self.loop_stack.push(SmJump {
            cont: None,
            brk: post,
            label: self.pending_label.take(),
            bare_break: true,
        });
        for ((entry, body), next) in bodies.into_iter().zip(next) {
            self.cur_state = entry;
            self.lower_seq(body);
            let mut fall = self.emit_goto(next);
            self.cur_buf.append(&mut fall);
            self.flush_cur();
        }
        self.loop_stack.pop();
        self.cur_state = post;
    }
}
