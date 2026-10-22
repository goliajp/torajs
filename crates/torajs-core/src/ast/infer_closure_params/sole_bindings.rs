//! Which bare names the whole program binds exactly once.
//!
//! The receiver table the other callback arms read is flat and keyed by
//! name, so two functions that each declare a `q` share one entry and
//! the one declared last answers for both. That is tolerable where a
//! wrong answer only loses a seed; an arm whose seed is a specific type
//! would instead hand a handler another binding's type, and the checker
//! then refuses a program bun runs (`q: Promise<string>` in one
//! function, `q: Promise<number>` in the next). A name bound once has
//! one declaration to read, wherever it is used.

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// The one declaration of a name.
#[derive(Clone, Copy)]
pub(in crate::ast) struct SoleDecl<'a> {
    pub ann: Option<&'a str>,
}

pub(in crate::ast) struct SoleBindings<'a> {
    seen: HashMap<&'a str, (usize, SoleDecl<'a>)>,
    /// every type parameter the program declares
    type_params: HashSet<&'a str>,
}

impl<'a> SoleBindings<'a> {
    pub(in crate::ast) fn collect(ast: &'a Ast) -> Self {
        let mut b = SoleBindings {
            seen: HashMap::new(),
            type_params: HashSet::new(),
        };
        for s in &ast.stmts {
            b.count_stmt(s);
        }
        b
    }

    /// The declaration of `name`, when the program has exactly one.
    pub(in crate::ast) fn sole(&self, name: &str) -> Option<SoleDecl<'a>> {
        match self.seen.get(name) {
            Some((1, d)) => Some(*d),
            _ => None,
        }
    }

    /// Does `ann` mention a type parameter outside `own`? Out of scope
    /// for a lifted handler, whichever function declared it.
    pub(in crate::ast) fn names_type_param(&self, ann: &str, own: &[String]) -> bool {
        ann.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .any(|w| self.type_params.contains(w) && !own.iter().any(|t| t == w))
    }

    fn bind(&mut self, name: &'a str, ann: Option<&'a String>) {
        let d = SoleDecl {
            ann: ann.map(String::as_str),
        };
        let e = self.seen.entry(name).or_insert((0, d));
        e.0 += 1;
        e.1 = d;
    }

    fn count_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::LetDecl { name, type_ann, .. } => self.bind(name, type_ann.as_ref()),
            Stmt::UsingDecl { name, .. } => self.bind(name, None),
            Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
                self.bind(var_name, None)
            }
            Stmt::YieldInto { var, .. } => self.bind(var, None),
            Stmt::Try {
                catch_param: Some(p),
                ..
            } => self.bind(p, None),
            Stmt::FnDecl {
                name,
                type_params,
                params,
                body,
                ..
            } => {
                self.bind(name, None);
                self.type_params
                    .extend(type_params.iter().map(String::as_str));
                for p in params {
                    self.bind(&p.name, p.type_ann.as_ref());
                }
                for b in body {
                    self.count_stmt(b);
                }
            }
            Stmt::ClassDecl {
                name, type_params, ..
            } => {
                self.bind(name, None);
                self.type_params
                    .extend(type_params.iter().map(String::as_str));
            }
            Stmt::TypeDecl { type_params, .. } => self
                .type_params
                .extend(type_params.iter().map(String::as_str)),
            Stmt::ImportDecl {
                namespace, named, ..
            } => {
                if let Some(ns) = namespace {
                    self.bind(ns, None);
                }
                for (orig, alias) in named {
                    self.bind(alias.as_ref().unwrap_or(orig), None);
                }
            }
            _ => {}
        }
        for c in stmt_children_ref(s) {
            self.count_stmt(c);
        }
    }
}
