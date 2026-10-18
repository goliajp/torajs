//! The statements of the walk: the place each statement puts its
//! expressions in, the view bindings it hands to the view scan, and the
//! function bodies it enters.

use crate::ast::{Expr, ExprId, Param, Stmt};

use super::super::program::Cat;
use super::super::view;
use super::{Ctx, Walk};

impl<'p, 'a> Walk<'p, 'a> {
    pub(super) fn stmts(&mut self, list: &'a [Stmt]) {
        let mut flat: Vec<&'a Stmt> = Vec::new();
        flatten(list, &mut flat);
        for (k, s) in flat.iter().enumerate() {
            if let Stmt::LetDecl {
                name,
                mutable: false,
                init,
                ..
            } = s
                && matches!(self.prog.ast.get_expr(*init), Expr::Index { .. })
                && self.cat(*init) == Some(Cat::Elem)
                && !self.exempt
            {
                match view::reads(self.prog, self.h, name, &flat[k + 1..]) {
                    Ok(n) => *self.view_reads.entry(name.as_str()).or_default() += n,
                    Err(e) => self.fail(e),
                }
            }
            self.stmt(s);
        }
    }

    fn one(&mut self, s: &'a Stmt) {
        self.stmts(std::slice::from_ref(s));
    }

    fn stmt(&mut self, s: &'a Stmt) {
        if self.err.is_some() {
            return;
        }
        match s {
            Stmt::Expr(e) => self.expr(*e, Ctx::Discard),
            Stmt::LetDecl {
                name,
                mutable,
                type_ann,
                init,
                ..
            } => self.let_decl(name, *mutable, type_ann.as_deref(), *init),
            Stmt::Return(Some(e)) => self.expr(*e, Ctx::Return),
            Stmt::Throw(e) | Stmt::Yield(e) => self.expr(*e, Ctx::Other),
            Stmt::YieldInto { value: e, .. } | Stmt::UsingDecl { init: e, .. } => {
                self.expr(*e, Ctx::Other)
            }
            Stmt::FnDecl {
                name, params, body, ..
            } => self.fn_decl(name, params, body),
            Stmt::Block(v) | Stmt::Multi(v) => self.stmts(v),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond, Ctx::Other);
                self.one(then_branch);
                if let Some(e) = else_branch {
                    self.one(e);
                }
            }
            Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
                self.expr(*cond, Ctx::Other);
                self.one(body);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                if let Some(i) = init {
                    if matches!(i.as_ref(), Stmt::LetDecl { init, .. } if self.cat(*init) == Some(Cat::Elem))
                    {
                        self.fail("a view bound in a `for` head".into());
                    }
                    self.one(i);
                }
                if let Some(c) = cond {
                    self.expr(*c, Ctx::Other);
                }
                if let Some(st) = step {
                    self.expr(*st, Ctx::Discard);
                }
                self.one(body);
            }
            Stmt::ForOf {
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                self.expr(*elem_expr, Ctx::Other);
                if let Some(o) = forin_obj {
                    self.expr(*o, Ctx::Other);
                }
                self.one(body);
            }
            Stmt::ForOfSplitIter {
                parent, sep, body, ..
            } => {
                self.expr(*parent, Ctx::Other);
                self.expr(*sep, Ctx::Other);
                self.one(body);
            }
            Stmt::Labeled { body, .. } => self.one(body),
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                self.expr(*scrutinee, Ctx::Other);
                for c in cases {
                    self.expr(c.value, Ctx::Other);
                    self.stmts(&c.body);
                }
                if let Some(d) = default {
                    self.stmts(d);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                self.stmts(body);
                self.stmts(catch_body);
                if let Some(f) = finally_body {
                    self.stmts(f);
                }
            }
            Stmt::ExportDecl {
                inner,
                default_expr,
                ..
            } => {
                if let Some(i) = inner {
                    self.one(i);
                }
                if let Some(e) = default_expr {
                    self.expr(*e, Ctx::Other);
                }
            }
            _ => {}
        }
    }

    fn let_decl(&mut self, name: &str, mutable: bool, ann: Option<&'a str>, init: ExprId) {
        let ast = self.prog.ast;
        let seed = name == "__this"
            && self
                .cur_fn
                .and_then(|f| f.strip_prefix("__new_"))
                .is_some_and(|c| self.h.containers.iter().any(|(n, _)| *n == c))
            && matches!(ast.get_expr(init), Expr::ObjectLit { fields }
                if fields.iter().all(|(_, v)| seed_value(ast.get_expr(*v))));
        if seed {
            let was = std::mem::replace(&mut self.exempt, true);
            self.expr(init, Ctx::Other);
            self.exempt = was;
            return;
        }
        // a slot typed as holding an instance takes nothing else
        if !self.exempt && ann.and_then(|a| self.h.ann_cat(a)).is_some() && self.cat(init).is_none()
        {
            self.fail(format!(
                "`{name}` is declared holding it but bound to another value"
            ));
        }
        self.expr(init, Ctx::Let { mutable, ann });
    }

    fn fn_decl(&mut self, name: &'a str, params: &'a [Param], body: &'a [Stmt]) {
        let c = self.h.class;
        let own = name.strip_prefix("__new_") == Some(c)
            || name
                .strip_prefix("__cm_")
                .and_then(|n| n.strip_suffix("__ctor"))
                == Some(c);
        let saved = (self.cur_fn, self.cur_ret, self.exempt);
        self.cur_fn = Some(name);
        self.cur_ret = self.watched.get(name).and_then(|w| w.ret);
        self.exempt |= own;
        self.enter_scope(params, body);
        for p in params {
            if let Some(d) = p.default {
                self.expr(d, Ctx::Other);
            }
        }
        self.stmts(body);
        self.leave_scope();
        (self.cur_fn, self.cur_ret, self.exempt) = saved;
    }
}

/// `Multi` shares its parent's scope: its statements belong to the
/// enclosing list.
fn flatten<'a>(list: &'a [Stmt], out: &mut Vec<&'a Stmt>) {
    for s in list {
        match s {
            Stmt::Multi(inner) => flatten(inner, out),
            _ => out.push(s),
        }
    }
}

/// What a container's factory seeds a field with before its
/// constructor runs.
fn seed_value(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Number(_) | Expr::Bool(_) | Expr::Null | Expr::Uninit | Expr::String(_)
    ) || matches!(e, Expr::Ident(n) if n == "undefined" || n.starts_with("__undef_slot__"))
}
