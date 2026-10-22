//! Receivers whose settled type is written on a declaration: a binding
//! annotated `Promise<T>`, and a call of a function whose return type
//! is written `Promise<T>`.

use super::super::infer_closure_params::SoleBindings;
use super::super::{Ast, Stmt};
use std::collections::HashMap;

/// What a receiver's inner type is read from.
pub(in crate::ast) struct PromiseTables<'a> {
    /// the flat receiver table — read for a `Promise.resolve(x)`
    /// argument and for whether a name is bound at all
    pub all_anns: &'a HashMap<String, String>,
    pub fn_ret_anns: &'a HashMap<String, Option<String>>,
    pub bindings: &'a SoleBindings<'a>,
}

impl PromiseTables<'_> {
    /// A binding annotated `Promise<T>` settles with a T, whatever
    /// initialized it. Read through the program-wide count of bindings,
    /// not the flat table, which keeps the last of several same-named
    /// declarations.
    pub(super) fn binding_inner(&self, name: &str) -> Option<String> {
        let inner = promise_inner(self.bindings.sole(name)?.ann?)?;
        (!self.bindings.names_type_param(&inner, &[])).then_some(inner)
    }

    /// A call of a function whose return type is written `Promise<T>`.
    pub(super) fn call_inner(&self, ast: &Ast, f: &str) -> Option<String> {
        let inner = declared_return(ast, f).and_then(promise_inner)?;
        (!self.bindings.names_type_param(&inner, &[])).then_some(inner)
    }
}

/// `T` of an annotation spelled exactly `Promise<T>`; `None` for a
/// union of promises, a subclass, or anything else whose settled value
/// the spelling does not name.
fn promise_inner(ann: &str) -> Option<String> {
    let inner = ann.trim().strip_prefix("Promise<")?.strip_suffix('>')?;
    // the `>` stripped must close the `Promise<` itself:
    // `Promise<A> | Promise<B>` ends in `>` too
    let mut depth = 0i32;
    for c in inner.chars() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then(|| inner.trim().to_string())
}

/// The written return type of the program's one non-generic function
/// `name`. A generic function's `Promise<T>` names a type parameter the
/// callback does not have in scope, and a sniffed return is a guess, so
/// neither answers.
fn declared_return<'a>(ast: &'a Ast, name: &str) -> Option<&'a str> {
    let mut found = None;
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name: n,
            type_params,
            return_type,
            ..
        } = s
            && n == name
        {
            if found.is_some() || !type_params.is_empty() {
                return None;
            }
            found = Some(return_type.as_deref()?);
        }
    }
    found
}
