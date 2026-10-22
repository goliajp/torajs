//! Receivers whose settled type is written on a declaration: a binding
//! annotated `Promise<T>`, and a call of a function whose return type
//! is written `Promise<T>`.

use super::super::desugar_with::walk::stmt_children_ref;
use super::super::infer_closure_params::SoleBindings;
use super::super::{Ast, ExprId, Stmt};
use crate::ast_desugar_builtin_new::promise::context::{promise_inner, settled};
use crate::check_type_of_call_promise_then::is_reject_mint;
use std::cell::Cell;
use std::collections::HashMap;

/// What a receiver's inner type is read from.
pub(in crate::ast) struct PromiseTables<'a> {
    /// the flat receiver table — read for a `Promise.resolve(x)`
    /// argument and for whether a name is bound at all
    pub all_anns: &'a HashMap<String, String>,
    pub fn_ret_anns: &'a HashMap<String, Option<String>>,
    pub bindings: &'a SoleBindings<'a>,
    /// how deep the reading of initializers and returns has gone; a
    /// binding whose initializer reads itself stops it
    depth: Cell<u32>,
}

/// How many initializers and returns one receiver reading follows.
const MAX_DEPTH: u32 = 32;

impl<'a> PromiseTables<'a> {
    pub(in crate::ast) fn new(
        all_anns: &'a HashMap<String, String>,
        fn_ret_anns: &'a HashMap<String, Option<String>>,
        bindings: &'a SoleBindings<'a>,
    ) -> Self {
        PromiseTables {
            all_anns,
            fn_ret_anns,
            bindings,
            depth: Cell::new(0),
        }
    }

    /// A binding annotated `Promise<T>` settles with a T, whatever
    /// initialized it. Read through the program-wide count of bindings,
    /// not the flat table, which keeps the last of several same-named
    /// declarations.
    pub(super) fn binding_inner(&self, ast: &Ast, name: &str) -> Option<String> {
        let d = self.bindings.sole(name)?;
        let inner = promise_inner(d.ann?)?;
        if d.init.is_some_and(|e| self.settles_any(ast, e)) {
            return Some("any".to_string());
        }
        primitive(&inner).then_some(inner)
    }

    /// Does the promise `e` evaluates to settle a value of any kind, as
    /// far as its spelling shows? A `Promise<T>` annotation over such a
    /// promise has no runtime meaning (§27.2.4.7 settles the value as it
    /// is), and the checker gives the binding or the call the promise's
    /// `Promise<any>` face (`check_stmt_let_decl::any_promise_slot`,
    /// `check_entry::hidden_any`), so a handler on it is seeded `any`.
    fn settles_any(&self, ast: &Ast, e: ExprId) -> bool {
        let d = self.depth.get();
        // a rejected promise fulfils with nothing (`never`)
        if d >= MAX_DEPTH || is_reject_mint(ast, e) {
            return false;
        }
        self.depth.set(d + 1);
        let any = super::resolve_promise_inner_ann(ast, e, self).is_some_and(|i| i == "any");
        self.depth.set(d);
        any
    }

    /// A call of a function whose return type is written `Promise<T>`.
    /// The function must be the program's only binding of `f`: a local
    /// `const f = …` or a parameter `f` in some other scope shadows the
    /// declaration there, and the call reads that binding instead.
    pub(super) fn call_inner(&self, ast: &Ast, f: &str) -> Option<String> {
        let d = self.bindings.sole(f)?;
        if d.ann.is_some() || d.init.is_some() {
            return None;
        }
        let (inner, body) = declared_return(ast, f)?;
        let inner = promise_inner(inner)?;
        // an async function settles what its body returns, as it is
        // (§27.7.5.3); only one whose returns the sniff proved are `T`
        // (`desugar_async::sniff`) settles a `T` the checker keeps
        // (`check_entry::hidden_any::promise_rets`) whatever its body
        // reads, so any other may settle a value of any kind
        if ast.async_fns.contains(f) {
            if !ast.async_proved_returns.contains(f) {
                return Some("any".to_string());
            }
        } else if (returns(body).into_iter())
            .filter(|e| !ast.synth_promise_static_calls.contains(e))
            .any(|e| self.settles_any(ast, e))
        {
            return Some("any".to_string());
        }
        primitive(&inner).then_some(inner)
    }
}

/// Is `inner` a type a handler can be seeded with from a binding's or a
/// return's annotation? Only a primitive: an object or array element
/// type reaches the handler with the widths of whatever built the
/// value, which the handler's own typed parameter does not follow —
/// `v[0].right` on a `Promise<{ right: number }[]>` read the raw bits
/// of 0.25.
fn primitive(inner: &str) -> bool {
    settled(inner).is_some()
}

/// The written return type of the program's one non-generic function
/// `name`. A generic function's `Promise<T>` names a type parameter the
/// callback does not have in scope, and a sniffed return is a guess, so
/// neither answers.
fn declared_return<'a>(ast: &'a Ast, name: &str) -> Option<(&'a str, &'a [Stmt])> {
    let mut found = None;
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name: n,
            type_params,
            return_type,
            body,
            ..
        } = s
            && n == name
        {
            if found.is_some() || !type_params.is_empty() {
                return None;
            }
            found = Some((return_type.as_deref()?, body.as_slice()));
        }
    }
    found
}

/// The values a function body returns; a nested function's returns are
/// its own.
fn returns(body: &[Stmt]) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::Return(Some(e)) => out.push(*e),
            Stmt::FnDecl { .. } => {}
            s => stack.extend(stmt_children_ref(s)),
        }
    }
    out
}
