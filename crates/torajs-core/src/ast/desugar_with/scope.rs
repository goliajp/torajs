//! Which names shadow the `with` object at a given point of the body.
//!
//! §9.1.1.2 puts the object environment record in the scope chain at
//! the position the `with` statement occupies. A name is captured by
//! the object only if nothing IN FRONT of that record already binds
//! it, so the whole question this file answers is: which records sit
//! in front here?
//!
//! Two scopes with opposite answers about `var`:
//!
//! - **The `with` body itself.** `var` hoists to the enclosing
//!   FUNCTION's variable environment, which sits BEHIND the object.
//!   So `with (o) { var v = 1 }` writes `o.v` when `o` carries `v` —
//!   the classic, and the reason `var` names are not binders here.
//! - **A nested function inside the body.** Its variable environment
//!   is created when it is called, IN FRONT of the object record it
//!   closed over. So its own `var`s, params and `arguments` all
//!   shadow, and only what it leaves free reaches the object.
//!
//! A binder is collected for the whole scope rather than from its
//! declaration onwards. That over-shadows a name declared in an inner
//! block and read before it (`with (o) { x; { let x = 1 } }`), which
//! is the shape 刀 1 shipped with; narrowing it to real block scopes
//! is its own change, not this one.

use std::collections::HashSet;

use super::walk::stmt_children_ref;
use crate::ast::{Param, Stmt};

pub(crate) struct Scope {
    bound: HashSet<String>,
    in_fn: bool,
}

impl Scope {
    /// The `with` body's own scope — `var` does NOT bind here.
    pub(crate) fn with_body(body: &[Stmt]) -> Self {
        let mut bound = HashSet::new();
        collect_binders(body, false, &mut bound);
        Scope {
            bound,
            in_fn: false,
        }
    }

    /// A function nested inside the body. Everything the enclosing
    /// scope shadowed still shadows (those records stay in front),
    /// plus this function's params, its `var`s and `arguments`.
    ///
    /// `arguments` is bound for an arrow too, where strictly it is the
    /// enclosing function's and so sits BEHIND the object. Rewriting
    /// it would hand a guarded conditional to the arguments-object
    /// pass, which reads a bare `arguments` occurrence; the deviation
    /// needs `with (o)` over an object carrying a property literally
    /// named `arguments`, read from an arrow.
    pub(crate) fn nested_fn(&self, params: &[Param], body: &[Stmt]) -> Self {
        let mut bound = self.bound.clone();
        for p in params {
            bound.insert(p.name.clone());
        }
        bound.insert("arguments".to_string());
        collect_binders(body, true, &mut bound);
        Scope { bound, in_fn: true }
    }

    pub(crate) fn shadows(&self, n: &str) -> bool {
        self.bound.contains(n)
    }

    /// True inside a nested function, where a `var` is an ordinary
    /// local rather than the hoisted-past-the-object shape the write
    /// knife still owes (RFC 20260814).
    pub(crate) fn in_nested_fn(&self) -> bool {
        self.in_fn
    }
}

fn collect_binders(stmts: &[Stmt], vars_bind: bool, out: &mut HashSet<String>) {
    for s in stmts {
        collect_binders_stmt(s, vars_bind, out);
    }
}

/// Walks statements only, and [`stmt_children_ref`] stops at a
/// `FnDecl` body — which is exactly right: a nested function's own
/// locals bind THERE, not here.
fn collect_binders_stmt(s: &Stmt, vars_bind: bool, out: &mut HashSet<String>) {
    match s {
        Stmt::LetDecl { name, is_var, .. } => {
            if !*is_var || vars_bind {
                out.insert(name.clone());
            }
        }
        Stmt::ClassDecl { name, .. } | Stmt::FnDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::UsingDecl { name, .. } => {
            out.insert(name.clone());
        }
        Stmt::ForOf {
            var_name,
            src_ident,
            i_ident,
            ..
        } => {
            // The parser already hoisted the source to `src_ident` and
            // minted the counter, so all three names are the loop's.
            out.insert(var_name.clone());
            out.insert(src_ident.clone());
            out.insert(i_ident.clone());
        }
        Stmt::ForOfSplitIter { var_name, .. } => {
            out.insert(var_name.clone());
        }
        Stmt::Try { catch_param, .. } => {
            if let Some(p) = catch_param {
                out.insert(p.clone());
            }
        }
        _ => {}
    }
    for c in stmt_children_ref(s) {
        collect_binders_stmt(c, vars_bind, out);
    }
}
