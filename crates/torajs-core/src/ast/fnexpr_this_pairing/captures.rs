//! Uses of the name inside a function body that declares none: the
//! binding a closure captured into that lifted body, or the top-level
//! declaration read from a top-level function.

use super::Pairing;
use crate::ast::{ExprId, Stmt};

/// The inits of `name`'s declarations directly in `list` (through
/// `Multi`, which shares the scope).
pub(super) fn direct_decl_inits(list: &[Stmt], name: &str, out: &mut Vec<ExprId>) {
    for s in list {
        match s {
            Stmt::LetDecl { name: n, init, .. } if n == name => out.push(*init),
            Stmt::Multi(inner) => direct_decl_inits(inner, name, out),
            _ => {}
        }
    }
}

/// Who a lifted closure's captured `name` comes from: a declaration
/// group, or the enclosing lifted body (a closure nested in a closure
/// re-captures it).
pub(super) enum Capture<'a> {
    Group(usize),
    Fn(&'a str),
}

impl<'a> Pairing<'a> {
    /// A closure capturing the name, met under `cur`: its lifted body
    /// reads the binding in scope here. A body captured from two
    /// places (a shared lifted name) cannot be paired.
    pub(super) fn capture(&mut self, fn_name: &'a str, cur: Option<usize>) {
        let from = match (cur, self.cur_fn) {
            (Some(g), _) => Capture::Group(g),
            (None, Some(f)) => Capture::Fn(f),
            (None, None) => {
                self.failed = true;
                return;
            }
        };
        if self.captured_by.insert(fn_name, from).is_some() {
            self.failed = true;
        }
    }

    /// Hand every use a lifted body made of a captured binding to the
    /// group it was captured from, following nested captures out; a
    /// top-level function that captured nothing reads the top-level
    /// declaration.
    pub(super) fn bind_captured_uses(
        &mut self,
        top_fns: &std::collections::HashSet<&str>,
        top: Option<usize>,
    ) {
        for (f, eid) in std::mem::take(&mut self.unbound) {
            let mut at = f;
            let mut group = None;
            for _ in 0..=self.captured_by.len() {
                match self.captured_by.get(at) {
                    Some(Capture::Group(g)) => {
                        group = Some(*g);
                        break;
                    }
                    Some(Capture::Fn(outer)) => at = outer,
                    None => {
                        group = top.filter(|_| top_fns.contains(at));
                        break;
                    }
                }
            }
            let Some(g) = group else {
                self.failed = true;
                return;
            };
            self.record_use(eid, Some(g));
        }
    }
}
