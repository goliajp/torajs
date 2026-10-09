//! How a branch of an `if` or a `switch` clause ends, for the union
//! narrows that read it (TS §4.24 control flow): the narrow after an
//! `if` keeps the side whose branch cannot complete, and a clause that
//! can complete carries its members into the next one.
//!
//! [`crate::check::stmt_diverges`] answers only "surely diverges" and
//! treats every form it does not know as completing. For a narrow that
//! is the wrong default: a branch that does diverge in a form it does
//! not know (`try { return } finally {}`) leaves its members in the
//! narrow, and a read TS accepts is refused. So the answer here has a
//! third value, and a caller that gets it gives up the narrow instead.

use crate::ast::{Ast, Expr, Stmt};

use super::{Checker, Type};

/// The way control leaves a statement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum End {
    /// Control never reaches the statement's end.
    Diverges,
    /// Control may reach the statement's end.
    Completes,
    /// A form this checker does not decide (a loop that may never
    /// exit, a labeled statement, a `switch`).
    Unknown,
}

impl End {
    /// Two paths, either of which may be taken.
    fn either(self, other: End) -> End {
        match (self, other) {
            (End::Diverges, End::Diverges) => End::Diverges,
            (End::Completes, _) | (_, End::Completes) => End::Completes,
            _ => End::Unknown,
        }
    }
}

impl Checker {
    /// How control leaves `s`.
    pub(crate) fn stmt_end(&self, ast: &Ast, s: &Stmt) -> End {
        match s {
            Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => End::Diverges,
            Stmt::Block(stmts) | Stmt::Multi(stmts) => self.block_end(ast, stmts),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                let other = match else_branch {
                    Some(eb) => self.stmt_end(ast, eb),
                    None => End::Completes,
                };
                self.stmt_end(ast, then_branch).either(other)
            }
            // ES §14.15.3: an abrupt finally replaces the completion;
            // a normal one keeps that of the try block or the catch
            Stmt::Try {
                body,
                had_catch,
                catch_body,
                finally_body,
                ..
            } => {
                let fin = finally_body
                    .as_deref()
                    .map_or(End::Completes, |f| self.block_end(ast, f));
                let caught = if *had_catch {
                    self.block_end(ast, catch_body)
                } else {
                    End::Diverges
                };
                match (fin, self.block_end(ast, body).either(caught)) {
                    (End::Diverges, _) | (_, End::Diverges) => End::Diverges,
                    (End::Completes, inner) => inner,
                    (End::Unknown, _) => End::Unknown,
                }
            }
            // a loop whose condition can be false completes when it is
            Stmt::While { cond, .. } => self.loop_end(ast, Some(*cond)),
            Stmt::For { cond, .. } => self.loop_end(ast, *cond),
            Stmt::ForOf { .. } | Stmt::ForOfSplitIter { .. } => End::Completes,
            Stmt::DoWhile { .. } | Stmt::Labeled { .. } => End::Unknown,
            Stmt::Switch { default, .. } => match default {
                // no clause need match
                None => End::Completes,
                Some(_) => End::Unknown,
            },
            Stmt::Expr(e) if self.never_returns(ast, *e) => End::Diverges,
            _ => End::Completes,
        }
    }

    fn block_end(&self, ast: &Ast, stmts: &[Stmt]) -> End {
        stmts
            .last()
            .map_or(End::Completes, |s| self.stmt_end(ast, s))
    }

    fn loop_end(&self, ast: &Ast, cond: Option<crate::ast::ExprId>) -> End {
        match cond.map(|c| ast.get_expr(c)) {
            None | Some(Expr::Bool(true)) => End::Unknown,
            Some(_) => End::Completes,
        }
    }

    /// A call whose callee is declared to return `never` (TS §4.24's
    /// assertion form; `process.exit` is declared so by Node), as an
    /// expression statement.
    fn never_returns(&self, ast: &Ast, e: crate::ast::ExprId) -> bool {
        let Expr::Call { callee, .. } = ast.get_expr(e) else {
            return false;
        };
        match ast.get_expr(*callee) {
            Expr::Ident(f) => {
                let ty = match self.scopes.iter().rev().find_map(|s| s.get(f)) {
                    Some(info) => Some(&info.ty),
                    None => self.globals.get(f),
                };
                matches!(ty, Some(Type::Function(_, r)) if **r == Type::Never)
            }
            Expr::Member { obj, name } => {
                name == "exit"
                    && matches!(ast.get_expr(*obj), Expr::Ident(p) if p == "process")
                    && self.scopes.iter().all(|s| !s.contains_key("process"))
                    && !self.globals.contains_key("process")
            }
            _ => false,
        }
    }
}
