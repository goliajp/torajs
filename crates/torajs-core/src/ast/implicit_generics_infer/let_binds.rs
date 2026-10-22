//! The binding → annotation map the return sniff reads.
//!
//! The map is flat, one entry per name, while `let` and `const` are
//! block scoped: in `let n = 3; { let n = x; return n; }` the return
//! reads the inner `n`. So a name keeps its entry only while every
//! declaration of it the body makes names the same type — counting a
//! declaration in a nested statement over a name the caller seeded (a
//! parameter, a capture) — and a declaration whose initializer the
//! sniff cannot name drops the name for the rest of the body.

use std::collections::{HashMap, HashSet};

use super::{AstExprsView, binds_to_params, infer_expr_ann_with};
use crate::ast::Stmt;

/// Register in `binds` each binding `body` declares, as above.
pub(super) fn collect(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut HashMap<String, String>,
    fn_sigs: &HashMap<String, String>,
) {
    let mut scan = Scan {
        exprs,
        fn_sigs,
        declared: HashSet::new(),
        dropped: HashSet::new(),
    };
    for s in body {
        scan.stmt(s, 0, binds);
    }
}

struct Scan<'a> {
    exprs: AstExprsView<'a>,
    fn_sigs: &'a HashMap<String, String>,
    /// the names a declaration of `body` has bound so far
    declared: HashSet<String>,
    /// the names whose declarations disagree or resist the sniff
    dropped: HashSet<String>,
}

impl Scan<'_> {
    /// `depth` counts the statements `s` sits in below the body.
    fn stmt(&mut self, s: &Stmt, depth: u32, binds: &mut HashMap<String, String>) {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => self.declare(name, type_ann.as_ref(), *init, depth, binds),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.stmt(then_branch, depth + 1, binds);
                if let Some(eb) = else_branch {
                    self.stmt(eb, depth + 1, binds);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                self.stmt(body, depth + 1, binds)
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    self.stmt(i, depth + 1, binds);
                }
                self.stmt(body, depth + 1, binds);
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                let depth = if matches!(s, Stmt::Block(_)) {
                    depth + 1
                } else {
                    depth
                };
                for s in stmts {
                    self.stmt(s, depth, binds);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                let blocks = [Some(body), Some(catch_body), finally_body.as_ref()];
                for s in blocks.into_iter().flatten().flatten() {
                    self.stmt(s, depth + 1, binds);
                }
            }
            _ => {}
        }
    }

    fn declare(
        &mut self,
        name: &str,
        written: Option<&String>,
        init: crate::ast::ExprId,
        depth: u32,
        binds: &mut HashMap<String, String>,
    ) {
        if self.dropped.contains(name) {
            return;
        }
        // an explicit annotation wins; else the initializer's shape, read
        // with the bindings registered so far (`let x = 3; let y = x + 1`)
        let ann = written.cloned().or_else(|| {
            let params = binds_to_params(binds);
            infer_expr_ann_with(self.exprs, init, &params, binds, self.fn_sigs)
        });
        let prior = binds.get(name);
        let shadows = self.declared.contains(name) || (depth > 0 && prior.is_some());
        self.declared.insert(name.to_string());
        match ann {
            Some(a) if !shadows || prior == Some(&a) => {
                binds.insert(name.to_string(), a);
            }
            _ => {
                binds.remove(name);
                self.dropped.insert(name.to_string());
            }
        }
    }
}
