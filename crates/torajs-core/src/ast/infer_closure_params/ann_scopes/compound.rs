//! The statements whose bindings sit in a scope of their own beside
//! their body's: a loop head, a catch clause, a switch's case block.

use super::Builder;
use crate::ast::Stmt;
use crate::ast::desugar_with::walk::stmt_exprs;

impl<'a> Builder<'a> {
    /// The head's bindings enclose the body (§14.7.4.2), and the head's
    /// own expressions read them.
    pub(super) fn for_loop(
        &mut self,
        s: &'a Stmt,
        init: Option<&'a Stmt>,
        body: &'a Stmt,
        scope: u32,
    ) {
        let hs = match init {
            Some(i) => self.block(std::slice::from_ref(i), scope),
            None => scope,
        };
        for e in stmt_exprs(s) {
            self.expr(e, hs);
        }
        self.block(std::slice::from_ref(body), hs);
    }

    pub(super) fn for_of(
        &mut self,
        var_name: &str,
        ann: Option<String>,
        body: &'a Stmt,
        scope: u32,
    ) {
        let hs = self.new_scope(Some(scope));
        self.bind(hs, var_name, ann);
        self.block(std::slice::from_ref(body), hs);
    }

    /// `catch` is the parameter, its annotation and the clause's body.
    pub(super) fn try_stmt(
        &mut self,
        body: &'a [Stmt],
        catch: (Option<&str>, Option<String>, &'a [Stmt]),
        finally: Option<&'a [Stmt]>,
        scope: u32,
    ) {
        let (param, ann, catch_body) = catch;
        self.block(body, scope);
        let cs = self.new_scope(Some(scope));
        if let Some(p) = param {
            self.bind(cs, p, ann);
        }
        self.block(catch_body, cs);
        if let Some(f) = finally {
            self.block(f, scope);
        }
    }

    /// Every clause shares the one CaseBlock scope (§14.12.2).
    pub(super) fn switch(&mut self, lists: Vec<&'a [Stmt]>, scope: u32) {
        let cs = self.new_scope(Some(scope));
        for list in &lists {
            self.declare(cs, list);
        }
        for st in lists.iter().flat_map(|l| l.iter()) {
            self.stmt(st, cs);
        }
    }
}
