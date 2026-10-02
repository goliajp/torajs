//! The compound statements of the walk: loops, labels, `try`,
//! `switch`.

use crate::ast::Stmt;

use super::scan::Scan;

impl Scan<'_, '_> {
    pub(super) fn flow(&mut self, s: &Stmt) -> bool {
        match s {
            Stmt::While { cond, body } => {
                self.looped(|sc| {
                    sc.expr(*cond);
                    sc.stmt(body);
                });
                true
            }
            Stmt::DoWhile { body, cond } => {
                self.looped(|sc| {
                    sc.stmt(body);
                    sc.expr(*cond);
                });
                true
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i);
                }
                self.looped(|sc| {
                    if let Some(c) = cond {
                        sc.expr(*c);
                    }
                    sc.stmt(body);
                    if let Some(st) = step {
                        sc.expr(*st);
                    }
                });
                true
            }
            Stmt::ForOf {
                elem_expr,
                body,
                forin_obj,
                ..
            } => {
                let protocol =
                    !self.indexable_source(*elem_expr) || self.env.shadow.iterator_hook_written();
                self.looped(|sc| {
                    if protocol {
                        sc.release();
                    }
                    sc.expr(*elem_expr);
                    if let Some(o) = forin_obj {
                        sc.expr(*o);
                    }
                    sc.stmt(body);
                });
                true
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.looped(|sc| {
                    sc.stmt(body);
                });
                true
            }
            Stmt::Labeled { body, .. } => {
                // `break label` on a non-loop statement lands right
                // after it
                let outer_jump = std::mem::replace(&mut self.jumped, false);
                self.stmt(body);
                self.released |= self.jumped;
                self.jumped = outer_jump;
                true
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => self.try_stmt(body, catch_body, finally_body.as_deref()),
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // clauses fall through into each other, and `break`
                // carries its release past the switch
                let outer_jump = std::mem::replace(&mut self.jumped, false);
                for c in cases {
                    self.expr(c.value);
                    self.seq_owned(&c.body);
                }
                if let Some(d) = default {
                    self.seq_owned(d);
                }
                self.released |= self.jumped;
                self.jumped = outer_jump;
                true
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.stmt(i);
                }
                if let Some(e) = default_expr {
                    self.expr(*e);
                }
                true
            }
            _ => true,
        }
    }

    /// The catch may start at any point of the body and the finally at
    /// any point of either, including after a branch that left by
    /// `return` or `throw`: both start from every release the region
    /// before them ran.
    fn try_stmt(&mut self, body: &[Stmt], catch_body: &[Stmt], fin: Option<&[Stmt]>) -> bool {
        let outer_ever = std::mem::replace(&mut self.ever, self.released);
        let body_ft = self.seq_owned(body);
        let after_body = self.released;
        let mut catch_ft = false;
        if !catch_body.is_empty() {
            self.released = self.ever;
            catch_ft = self.seq_owned(catch_body);
        }
        let fin_ft = match fin {
            Some(f) => {
                let out = (body_ft && after_body) || (catch_ft && self.released);
                self.released = self.ever;
                let ft = self.seq_owned(f);
                self.released |= out;
                ft
            }
            None => {
                self.released = (body_ft && after_body) || (catch_ft && self.released);
                true
            }
        };
        self.ever |= outer_ever;
        (body_ft || catch_ft) && fin_ft
    }
}
