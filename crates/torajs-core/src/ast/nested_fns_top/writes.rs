//! Which of a scope's own names something in the scope writes.
//!
//! `var_shared_fn_names` asks it of a function body or a script's top
//! list, and `fndecl_rebound` of every list whose declarations it may
//! give a slot. A write that lands on the scope's binding for `z`
//! makes `function z() {}` a written binding (shared with a `var z`
//! when there is one). A write an inner scope resolves to a binding of
//! its own does not: a nested function's parameter, `var` or `let`
//! named `z`, a block's `let z`, a catch parameter, a `for (let z …)`
//! head. Counting those too sent the declaration to the untyped lane
//! for nothing.
//!
//! A statement shape the walk does not model (a class body, an import
//! or export) is scanned without regard to scope, which can only
//! answer "written" too often.

use std::collections::HashSet;

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// The names `list` (a function body or a script's top level) writes
/// with `=`, a compound assignment or `++` / `--`, where the write
/// resolves to a binding of the list's own scope or one outside it.
pub(in crate::ast) fn scope_writes(ast: &Ast, list: &[Stmt]) -> HashSet<String> {
    let mut w = Walk {
        ast,
        shadow: Vec::new(),
        out: HashSet::new(),
    };
    for s in list {
        w.stmt(s);
    }
    w.out
}

struct Walk<'a> {
    ast: &'a Ast,
    /// Every name an inner scope around the walk's position binds.
    shadow: Vec<String>,
    out: HashSet<String>,
}

impl Walk<'_> {
    /// A block-shaped list: its lexical declarations bind inside it.
    fn block(&mut self, list: &[Stmt]) {
        let mark = self.shadow.len();
        lexical_names(list, &mut self.shadow);
        for s in list {
            self.stmt(s);
        }
        self.shadow.truncate(mark);
    }

    /// A statement in a position of its own (a branch, a loop body).
    fn nested(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(list) => self.block(list),
            _ => {
                let mark = self.shadow.len();
                self.stmt(s);
                self.shadow.truncate(mark);
            }
        }
    }

    /// A function's parameters and body: every name its own scope
    /// declares hides the outer binding of that name.
    fn function(&mut self, params: &[Param], body: &[Stmt]) {
        let mark = self.shadow.len();
        self.shadow.extend(params.iter().map(|p| p.name.clone()));
        let mut vars = std::collections::HashMap::new();
        crate::ast::var_hoist::count_var_names(body, &mut vars);
        self.shadow.extend(vars.into_keys());
        lexical_names(body, &mut self.shadow);
        for d in params.iter().filter_map(|p| p.default) {
            self.expr(d);
        }
        for s in body {
            self.stmt(s);
        }
        self.shadow.truncate(mark);
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) | Stmt::Return(Some(e)) => {
                self.expr(*e)
            }
            Stmt::YieldInto { value, .. } => self.expr(*value),
            Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => self.expr(*init),
            Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::TypeDecl { .. } => {}
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.nested(then_branch);
                if let Some(e) = else_branch {
                    self.nested(e);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.expr(*cond);
                self.nested(body);
            }
            Stmt::Labeled { body, .. } => self.nested(body),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let mark = self.shadow.len();
                if let Some(i) = init {
                    if let Stmt::LetDecl {
                        name,
                        is_var: false,
                        ..
                    } = &**i
                    {
                        self.shadow.push(name.clone());
                    }
                    self.stmt(i);
                }
                if let Some(c) = cond {
                    self.expr(*c);
                }
                if let Some(st) = step {
                    self.expr(*st);
                }
                self.nested(body);
                self.shadow.truncate(mark);
            }
            // a `var` / bare head binds a fresh loop temp and assigns
            // the user's name in the body, which the walk sees there
            Stmt::ForOf {
                var_name,
                elem_expr,
                body,
                forin_obj,
                ..
            } => {
                self.expr(*elem_expr);
                if let Some(o) = forin_obj {
                    self.expr(*o);
                }
                self.shadow.push(var_name.clone());
                self.nested(body);
                self.shadow.pop();
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent);
                self.expr(*sep);
                self.shadow.push(var_name.clone());
                self.nested(body);
                self.shadow.pop();
            }
            Stmt::Block(list) => self.block(list),
            // the same scope as the statement around it
            Stmt::Multi(list) => {
                for s in list {
                    self.stmt(s);
                }
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee);
                // §14.12.4: one declarative environment for every clause
                let mark = self.shadow.len();
                for c in cases {
                    lexical_names(&c.body, &mut self.shadow);
                }
                if let Some(d) = default {
                    lexical_names(d, &mut self.shadow);
                }
                for c in cases {
                    self.expr(c.value);
                    for s in &c.body {
                        self.stmt(s);
                    }
                }
                for s in default.iter().flatten() {
                    self.stmt(s);
                }
                self.shadow.truncate(mark);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(body);
                let mark = self.shadow.len();
                self.shadow.extend(catch_param.iter().cloned());
                self.block(catch_body);
                self.shadow.truncate(mark);
                if let Some(f) = finally_body {
                    self.block(f);
                }
            }
            Stmt::FnDecl { params, body, .. } => self.function(params, body),
            _ => self.unscoped(s),
        }
    }

    fn expr(&mut self, e: ExprId) {
        match self.ast.get_expr(e) {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                if let Expr::Ident(n) = self.ast.get_expr(*target)
                    && !self.shadow.contains(n)
                {
                    self.out.insert(n.clone());
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                self.function(params, body);
                return;
            }
            _ => {}
        }
        for c in expr_children(self.ast, e) {
            self.expr(c);
        }
    }

    /// Every write under `s`, shadowing ignored.
    fn unscoped(&mut self, s: &Stmt) {
        let shadow = std::mem::take(&mut self.shadow);
        for e in stmt_exprs(s) {
            self.expr(e);
        }
        for c in stmt_children_ref(s) {
            self.stmt(c);
        }
        self.shadow = shadow;
    }
}

/// The names `list` declares lexically at its own level: `let`,
/// `const`, `using`, a class, and a function declaration (block-scoped
/// in a block; in a function body the caller is asking about these
/// names, so listing them changes nothing there).
fn lexical_names(list: &[Stmt], out: &mut Vec<String>) {
    for s in list {
        match s {
            Stmt::LetDecl {
                name,
                is_var: false,
                ..
            }
            | Stmt::UsingDecl { name, .. }
            | Stmt::ClassDecl { name, .. }
            | Stmt::FnDecl { name, .. } => out.push(name.clone()),
            Stmt::Multi(inner) => lexical_names(inner, out),
            _ => {}
        }
    }
}
