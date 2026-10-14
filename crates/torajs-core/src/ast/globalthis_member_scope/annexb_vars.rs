//! The block-nested function declarations of one function body (or of
//! the script) that §B.3.3.1 (§B.3.3.2 at the top level) also gives a
//! `var` binding in that body's scope. Every other declaration below
//! the body's own statement list binds only the block it sits in:
//! a block function in strict code, a generator or async one, and one
//! that a `let` / `const` / `class`, a loop head, or a function of an
//! enclosing block already declares (replacing it with `var F` would
//! be an early error there).

use super::super::annexb_fn_var::annexb_applies_at;
use super::super::{Ast, Stmt};

/// The watched names among them, for the function body `body`.
pub(super) fn annexb_var_names(
    ast: &Ast,
    body: &[Stmt],
    watch: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut w = Lists {
        ast,
        watch,
        encl: Vec::new(),
        out: Vec::new(),
    };
    for s in body {
        if !matches!(s, Stmt::FnDecl { .. }) {
            w.descend(s);
        }
    }
    w.out
}

struct Lists<'a> {
    ast: &'a Ast,
    watch: &'a std::collections::HashSet<String>,
    /// The lexical names of the scopes between the body and the
    /// statement the walk stands on.
    encl: Vec<String>,
    out: Vec<String>,
}

impl Lists<'_> {
    /// A statement list that is a block scope of its own.
    fn list(&mut self, stmts: &[&Stmt]) {
        let mark = self.encl.len();
        let mut fns = Vec::new();
        for s in stmts {
            lexical(s, &mut self.encl, &mut fns);
        }
        for s in stmts {
            self.member(s, &fns);
        }
        self.encl.truncate(mark);
    }

    /// `fns`: the functions the list itself declares, which are lexical
    /// to what lies deeper but not to each other (§B.3.3.4).
    fn member(&mut self, s: &Stmt, fns: &[String]) {
        match s {
            Stmt::FnDecl {
                name,
                span,
                is_generator,
                ..
            } => {
                if self.watch.contains(name)
                    && !*is_generator
                    && !self.ast.async_fns.contains(name)
                    && !self.ast.async_generator_fns.contains(name)
                    && !self.encl.iter().any(|n| n == name)
                    && annexb_applies_at(self.ast, name, *span)
                {
                    self.out.push(name.clone());
                }
            }
            Stmt::Multi(v) => {
                for c in v {
                    self.member(c, fns);
                }
            }
            _ => {
                let mark = self.encl.len();
                self.encl.extend(fns.iter().cloned());
                self.descend(s);
                self.encl.truncate(mark);
            }
        }
    }

    /// The scopes a statement opens below itself.
    fn descend(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(v) => self.list(&v.iter().collect::<Vec<_>>()),
            Stmt::Multi(v) => {
                for c in v {
                    self.descend(c);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                // the clauses share one block (§14.12.4)
                let all: Vec<&Stmt> = cases
                    .iter()
                    .flat_map(|c| c.body.iter())
                    .chain(default.iter().flatten())
                    .collect();
                self.list(&all);
            }
            // §B.3.5: a catch parameter does not block the `var`
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                for b in [Some(body), Some(catch_body), finally_body.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    self.list(&b.iter().collect::<Vec<_>>());
                }
            }
            Stmt::For { init, body, .. } => {
                let mark = self.encl.len();
                if let Some(i) = init {
                    lexical(i, &mut self.encl, &mut Vec::new());
                }
                self.list(&[body.as_ref()]);
                self.encl.truncate(mark);
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                self.encl.push(var_name.clone());
                self.list(&[body.as_ref()]);
                self.encl.pop();
            }
            // §B.3.4: a function as an `if` clause is in a block of its own
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.list(&[then_branch.as_ref()]);
                if let Some(e) = else_branch {
                    self.list(&[e.as_ref()]);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                self.list(&[body.as_ref()])
            }
            _ => {}
        }
    }
}

/// The lexical declarations a list member makes in the list's scope;
/// its function declarations go to `fns`.
fn lexical(s: &Stmt, names: &mut Vec<String>, fns: &mut Vec<String>) {
    match s {
        Stmt::LetDecl {
            name,
            is_var: false,
            ..
        }
        | Stmt::UsingDecl { name, .. }
        | Stmt::ClassDecl { name, .. } => names.push(name.clone()),
        Stmt::FnDecl { name, .. } => fns.push(name.clone()),
        Stmt::Multi(v) => {
            for c in v {
                lexical(c, names, fns);
            }
        }
        _ => {}
    }
}
