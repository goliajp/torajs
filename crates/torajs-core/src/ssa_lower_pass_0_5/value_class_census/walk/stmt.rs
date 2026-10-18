//! The statements of the walk: the place each statement puts its
//! expressions in, the view bindings it hands to the view scan, and the
//! function bodies it enters.

use std::collections::HashMap;

use crate::ast::{Expr, ExprId, Param, Stmt};

use super::super::program::Cat;
use super::super::view;
use super::{Ctx, Walk};

/// Each view rescans the rest of its block, so a block with more views
/// than this is judged as a failure rather than scanned in square time.
const MAX_VIEWS_PER_BLOCK: usize = 32;

impl<'p, 'a> Walk<'p, 'a> {
    pub(super) fn stmts(&mut self, list: &'a [Stmt]) {
        let mut flat: Vec<&'a Stmt> = Vec::new();
        flatten(list, &mut flat);
        let mut block = HashMap::new();
        for s in &flat {
            for name in view::declared(s) {
                block.insert(name, self.view_init(s));
            }
        }
        if block.values().flatten().count() > MAX_VIEWS_PER_BLOCK {
            self.fail(format!(
                "more than {MAX_VIEWS_PER_BLOCK} views in one block"
            ));
            return;
        }
        self.blocks.push(block);
        for (k, s) in flat.iter().enumerate() {
            if let (Some(init), Stmt::LetDecl { name, .. }) = (self.view_init(s), s) {
                match view::reads(self.prog, self.h, name, &flat[k + 1..]) {
                    Ok(n) => *self.view_reads.entry(init).or_default() += n,
                    Err(e) => self.fail(e),
                }
            }
            self.stmt(s);
        }
        self.blocks.pop();
    }

    /// The init of `s` when it binds a `const t = xs[i]` view.
    fn view_init(&self, s: &Stmt) -> Option<ExprId> {
        match s {
            Stmt::LetDecl {
                mutable: false,
                init,
                ..
            } if matches!(self.prog.ast.get_expr(*init), Expr::Index { .. })
                && self.cat(*init) == Some(Cat::Elem)
                && !self.exempt =>
            {
                Some(*init)
            }
            _ => None,
        }
    }

    /// Run `f` with `names` bound to no view.
    fn bound(&mut self, names: Vec<&'a str>, f: impl FnOnce(&mut Self)) {
        self.blocks
            .push(names.into_iter().map(|n| (n, None)).collect());
        f(self);
        self.blocks.pop();
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
                let head = init.as_deref().map(view::declared).unwrap_or_default();
                self.bound(head, |w| {
                    if let Some(i) = init {
                        if matches!(i.as_ref(), Stmt::LetDecl { init, .. } if w.cat(*init) == Some(Cat::Elem))
                        {
                            w.fail("a view bound in a `for` head".into());
                        }
                        w.stmt(i);
                    }
                    if let Some(c) = cond {
                        w.expr(*c, Ctx::Other);
                    }
                    if let Some(st) = step {
                        w.expr(*st, Ctx::Discard);
                    }
                    w.one(body);
                });
            }
            Stmt::ForOf {
                var_name,
                elem_expr,
                forin_obj,
                body,
                ..
            } => {
                if let Some(o) = forin_obj {
                    self.expr(*o, Ctx::Other);
                }
                self.bound(vec![var_name], |w| {
                    w.expr(*elem_expr, Ctx::Other);
                    w.one(body);
                });
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                self.expr(*parent, Ctx::Other);
                self.expr(*sep, Ctx::Other);
                self.bound(vec![var_name], |w| w.one(body));
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
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.stmts(body);
                let param = catch_param.as_deref().into_iter().collect();
                self.bound(param, |w| w.stmts(catch_body));
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
