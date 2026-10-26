//! The gate's census: every async declaration and every class at any
//! depth of the program, judged where it stands.

use super::{MethodKey, method_moves, stmt_for_awaits};
use crate::ast::desugar_with::walk;
use crate::ast::{Ast, Expr, StaticInit, Stmt, mangle_key};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct Census {
    /// Per declaration name: whether every async declaration of it
    /// moves, and whether one of them has a `for await`.
    fns: HashMap<String, (bool, bool)>,
    pub(super) methods: HashSet<MethodKey>,
    /// A moved method or declaration has a `for await`.
    pub(super) for_await: bool,
}

impl Census {
    /// Judge the top-level statement `s` and everything nested in it.
    pub(super) fn top(&mut self, ast: &Ast, s: &Stmt) {
        each_stmt(ast, s, &mut |s| self.judge(ast, s));
    }

    fn judge(&mut self, ast: &Ast, s: &Stmt) {
        match s {
            Stmt::FnDecl {
                name,
                body,
                is_generator,
                ..
            } if ast.async_fns.contains(name) && !ast.async_generator_fns.contains(name) => {
                let e = self.fns.entry(name.clone()).or_insert((true, false));
                e.0 &= !is_generator;
                e.1 |= body.iter().any(stmt_for_awaits);
            }
            Stmt::ClassDecl {
                decl_id,
                methods,
                static_methods,
                ..
            } => {
                for (is_static, list) in [(false, methods), (true, static_methods)] {
                    for m in list {
                        let key = (*decl_id, is_static, mangle_key(&m.name).into_owned());
                        if ast.async_ssa.async_methods.contains(&key) && method_moves(m) {
                            self.for_await |= m.body.iter().any(stmt_for_awaits);
                            self.methods.insert(key);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The declaration names that move: every async declaration of the
    /// name does.
    pub(super) fn names(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for (name, (all_move, for_await)) in &self.fns {
            if *all_move {
                self.for_await |= for_await;
                out.push(name.clone());
            }
        }
        out
    }
}

/// Call `f` on `s` and on every statement nested in it: the bodies of
/// declarations, of class members (static blocks included) and of
/// function values, and every block.
pub(super) fn each_stmt<'a>(ast: &'a Ast, s: &'a Stmt, f: &mut dyn FnMut(&'a Stmt)) {
    f(s);
    match s {
        Stmt::FnDecl { body, .. } => each_list(ast, body, f),
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                each_list(ast, &c.body, f);
            }
            for m in methods.iter().chain(static_methods) {
                each_list(ast, &m.body, f);
            }
            for init in static_init {
                if let StaticInit::Block(b) = init {
                    each_list(ast, b, f);
                }
            }
        }
        _ => {
            for arrow in walk::arrow_nodes(ast, s) {
                if let Expr::ArrowFn { body, .. } = ast.get_expr(arrow) {
                    each_list(ast, body, f);
                }
            }
            for c in walk::stmt_children_ref(s) {
                each_stmt(ast, c, f);
            }
        }
    }
}

fn each_list<'a>(ast: &'a Ast, body: &'a [Stmt], f: &mut dyn FnMut(&'a Stmt)) {
    for s in body {
        each_stmt(ast, s, f);
    }
}
