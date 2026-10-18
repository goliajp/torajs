//! What each binding holds, by function scope.
//!
//! A binding's value reaches every read of it, so a binding that holds
//! an instance must be read as one at every read: the checker types
//! some reads `any` (a top-level binding read in a loop body, for one),
//! and such a read hands the instance to the `any` world without any
//! expression of the walk holding it. A scope is a function's own
//! parameters and every `let` of its body outside nested functions; a
//! name bound twice in one scope with different readings is judged as
//! a failure.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::ast::{Param, Stmt};

use super::super::super::live_field_census::walk::nested_stmts;
use super::super::program::Cat;
use super::Walk;

/// Per bound name, what it holds (`None`: nothing of the class).
pub(super) type Scope<'a> = HashMap<&'a str, Option<Cat>>;

impl<'p, 'a> Walk<'p, 'a> {
    pub(super) fn enter_scope(&mut self, params: &'a [Param], body: &'a [Stmt]) {
        let mut scope = Scope::new();
        let mut clash = None;
        let mut bind = |name: &'a str, cat: Option<Cat>| match scope.entry(name) {
            Entry::Vacant(v) => {
                v.insert(cat);
            }
            Entry::Occupied(o) => {
                if *o.get() != cat {
                    clash = Some(name);
                }
            }
        };
        for p in params {
            bind(
                &p.name,
                p.type_ann.as_deref().and_then(|a| self.h.ann_cat(a)),
            );
        }
        let mut stack: Vec<&'a Stmt> = body.iter().rev().collect();
        while let Some(s) = stack.pop() {
            match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    ..
                } => {
                    let cat = (type_ann.as_deref())
                        .and_then(|a| self.h.ann_cat(a))
                        .or_else(|| self.cat(*init));
                    bind(name, cat);
                }
                Stmt::UsingDecl { name, .. }
                | Stmt::YieldInto { var: name, .. }
                | Stmt::ForOf { var_name: name, .. }
                | Stmt::ForOfSplitIter { var_name: name, .. } => bind(name, None),
                Stmt::Try {
                    catch_param: Some(c),
                    ..
                } => bind(c, None),
                _ => {}
            }
            nested_stmts(s, &mut |st| stack.push(st));
        }
        if let Some(n) = clash
            && !self.exempt
        {
            self.fail(format!(
                "`{n}` is bound twice in one scope with different readings"
            ));
        }
        self.scopes.push(scope);
        let names = params.iter().map(|p| (p.name.as_str(), None)).collect();
        self.blocks.push(names);
    }

    pub(super) fn leave_scope(&mut self) {
        self.scopes.pop();
        self.blocks.pop();
    }

    /// What the binding a read of `name` resolves to holds.
    pub(super) fn binding(&self, name: &str) -> Option<Cat> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .copied()
            .flatten()
    }
}
