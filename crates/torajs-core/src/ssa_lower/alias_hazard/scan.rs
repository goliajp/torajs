//! The evaluation-order walk over one binding's scope: does a read
//! of it follow something that may release its source?

use crate::ast::Stmt;

use super::Env;

pub(super) struct Scan<'e, 'a> {
    pub(super) env: &'e Env<'a>,
    pub(super) name: &'e str,
    pub(super) root: Option<String>,
    /// Something that may release a slot has run on some path to here.
    pub(super) released: bool,
    /// A release has run anywhere in the region being walked, including
    /// on paths that left it by `return` / `throw` — what a `catch` or a
    /// `finally` may start from.
    pub(super) ever: bool,
    /// Releases carried out of the innermost loop by `break` /
    /// `continue` (and out of every loop by a labeled jump).
    pub(super) jumped: bool,
    pub(super) unsafe_read: bool,
    /// A closure built in scope names the binding and may read it after
    /// any release that follows.
    pub(super) captured: bool,
}

impl<'e, 'a> Scan<'e, 'a> {
    pub(super) fn new(env: &'e Env<'a>, name: &'e str, root: Option<String>) -> Self {
        Scan {
            env,
            name,
            root,
            released: false,
            ever: false,
            jumped: false,
            unsafe_read: false,
            captured: false,
        }
    }

    pub(super) fn verdict(&self) -> bool {
        self.unsafe_read || (self.captured && self.ever)
    }

    pub(super) fn read(&mut self) {
        if self.released {
            self.unsafe_read = true;
        }
    }

    pub(super) fn release(&mut self) {
        self.released = true;
        self.ever = true;
    }

    /// Walk `list` in order; `false` once a statement leaves the
    /// binding's scope for good.
    pub(super) fn seq(&mut self, list: &[&Stmt]) -> bool {
        list.iter().all(|s| self.stmt(s))
    }

    pub(super) fn seq_owned(&mut self, list: &[Stmt]) -> bool {
        list.iter().all(|s| self.stmt(s))
    }

    /// A loop body runs again after its own tail: two passes reach the
    /// fixed point of a single monotone flag.
    pub(super) fn looped(&mut self, mut body: impl FnMut(&mut Self)) {
        let outer_jump = std::mem::replace(&mut self.jumped, false);
        body(self);
        self.released |= self.jumped;
        body(self);
        self.released |= self.jumped;
        self.jumped |= outer_jump;
    }

    pub(super) fn stmt(&mut self, s: &Stmt) -> bool {
        match s {
            Stmt::Expr(e) | Stmt::LetDecl { init: e, .. } => {
                self.expr(*e);
                true
            }
            Stmt::UsingDecl { init, .. } => {
                self.expr(*init);
                self.release();
                true
            }
            Stmt::Throw(e) => {
                self.expr(*e);
                false
            }
            Stmt::Return(v) => {
                if let Some(e) = v {
                    self.expr(*e);
                }
                false
            }
            Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
                self.expr(*e);
                self.release();
                true
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                let before = self.released;
                let then_ft = self.stmt(then_branch);
                let then_rel = self.released;
                self.released = before;
                let else_ft = else_branch.as_ref().is_none_or(|b| self.stmt(b));
                let else_rel = self.released;
                self.released = (then_ft && then_rel) || (else_ft && else_rel);
                then_ft || else_ft
            }
            // a labeled jump that leaves several loops is handed outward
            // by each loop it crosses
            Stmt::Break(_) | Stmt::Continue(_) => {
                self.jumped |= self.released;
                false
            }
            Stmt::Block(b) | Stmt::Multi(b) => self.seq_owned(b),
            // a nested declaration may read the binding whenever it is
            // called
            Stmt::FnDecl { body, .. } => {
                if super::mentions::list(self.env.ast, body, self.name) {
                    self.read();
                    self.captured = true;
                }
                true
            }
            Stmt::ClassDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => true,
            _ => self.flow(s),
        }
    }
}
