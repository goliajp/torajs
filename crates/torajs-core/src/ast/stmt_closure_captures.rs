//! The capture names of every closure minted anywhere inside one
//! STATEMENT — the statement-level counterpart of
//! [`super::nested_closure_captures`], which answers the same question
//! for one expression tree.
//!
//! The two forward-binding hoists (checker `check_hoist_closure_lets`,
//! lowering `hoist_forward_boxes`) used to ask that question only of
//! let-INITS, so a closure minted from any other statement position
//! never registered its captures and a binding the same block declares
//! later stayed invisible to it:
//!
//! ```text
//! var x = "outside";
//! var probe;
//! { probe = function () { return x; }; let x = "inside"; }
//! probe()   // node: "inside" — tr answered "outside"
//! ```
//!
//! §8.2.6 BlockDeclarationInstantiation creates a binding for every
//! lexical declaration of a block WHEN THE BLOCK IS ENTERED, so the
//! closure minted on the first statement already names the block's
//! `x`, not the outer one. The assignment is the whole difference from
//! the shape that already worked (`let probe = function () …`): both
//! mint the same closure over the same binding, and only the one
//! written as a declaration was being looked at. The same hole swallows
//! a closure passed as a call argument, written into an `if` condition,
//! or minted inside a nested block of the same region.
//!
//! **A region that binds the name answers the capture itself.** A
//! nested scope runs its own copy of this hoist when it lowers, so its
//! captures are filtered against what it declares before they reach
//! the enclosing list. Erring toward filtering is deliberate: an
//! over-reported name opens a provisional `Any` box for a binding that
//! did not need one, while an under-reported one is only the behaviour
//! that stood before this walk existed.

use super::nested_closure_captures;
use super::stmt::SwitchCase;
use super::{Ast, Stmt};

/// Push every capture name of every closure minted inside `s` that `s`
/// does not itself bind, in encounter order with duplicates — callers
/// use set semantics.
pub(crate) fn collect_stmt<'a>(ast: &'a Ast, s: &'a Stmt, out: &mut Vec<&'a str>) {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) | Stmt::Yield(e) => {
            nested_closure_captures::collect(ast, *e, out)
        }
        Stmt::Return(e) => {
            if let Some(e) = e {
                nested_closure_captures::collect(ast, *e, out);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            nested_closure_captures::collect(ast, *init, out)
        }
        Stmt::YieldInto { value, .. } => nested_closure_captures::collect(ast, *value, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            nested_closure_captures::collect(ast, *cond, out);
            collect_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            nested_closure_captures::collect(ast, *cond, out);
            collect_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_stmt(ast, body, out);
            nested_closure_captures::collect(ast, *cond, out);
        }
        // §14.12.4 gives the whole CaseBlock ONE declarative
        // environment, entered before the first case value is
        // evaluated — so the compare chain is inside the region and
        // only the scrutinee is outside it.
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            nested_closure_captures::collect(ast, *scrutinee, out);
            case_block(ast, cases, default.as_deref(), out);
        }
        // The head's binding and the body are one region (§14.7.4 —
        // the loop environment holds the `let`).
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let mut inner: Vec<&'a str> = Vec::new();
            let mut bound: Vec<&'a str> = Vec::new();
            if let Some(i) = init {
                collect_stmt(ast, i, &mut inner);
                lexical_names(std::slice::from_ref(&**i), &mut bound);
            }
            for e in [cond, step].into_iter().flatten() {
                nested_closure_captures::collect(ast, *e, &mut inner);
            }
            collect_stmt(ast, body, &mut inner);
            drain_unbound(inner, &bound, out);
        }
        Stmt::ForOf {
            var_name,
            src_ident,
            i_ident,
            elem_expr,
            body,
            forin_obj,
            ..
        } => {
            let mut inner: Vec<&'a str> = Vec::new();
            nested_closure_captures::collect(ast, *elem_expr, &mut inner);
            if let Some(o) = forin_obj {
                nested_closure_captures::collect(ast, *o, &mut inner);
            }
            collect_stmt(ast, body, &mut inner);
            drain_unbound(
                inner,
                &[var_name.as_str(), src_ident.as_str(), i_ident.as_str()],
                out,
            );
        }
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => {
            let mut inner: Vec<&'a str> = Vec::new();
            for e in [parent, sep] {
                nested_closure_captures::collect(ast, *e, &mut inner);
            }
            collect_stmt(ast, body, &mut inner);
            drain_unbound(inner, &[var_name.as_str()], out);
        }
        // Three regions, and the catch parameter binds in exactly one.
        Stmt::Try {
            body,
            catch_param,
            catch_body,
            finally_body,
            ..
        } => {
            region(ast, body, &[], out);
            let cp: Vec<&'a str> = catch_param.iter().map(String::as_str).collect();
            region(ast, catch_body, &cp, out);
            if let Some(fb) = finally_body {
                region(ast, fb, &[], out);
            }
        }
        Stmt::Block(b) => region(ast, b, &[], out),
        // A grouping that shares the surrounding scope, so it does not
        // filter — the holding list's own hoist owns these names.
        Stmt::Multi(b) => {
            for s in b {
                collect_stmt(ast, s, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_stmt(ast, body, out),
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(i) = inner {
                collect_stmt(ast, i, out);
            }
            if let Some(e) = default_expr {
                nested_closure_captures::collect(ast, *e, out);
            }
        }
        // A function body is its own scope with its own hoist, and a
        // declaration still standing here after the nested-fn passes
        // captures nothing from this list. Everything else binds no
        // closure.
        Stmt::FnDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_) => {}
    }
}

/// One clause list per case plus the default, all sharing the single
/// CaseBlock environment.
fn case_block<'a>(
    ast: &'a Ast,
    cases: &'a [SwitchCase],
    default: Option<&'a [Stmt]>,
    out: &mut Vec<&'a str>,
) {
    let mut inner: Vec<&'a str> = Vec::new();
    let mut bound: Vec<&'a str> = Vec::new();
    for c in cases {
        nested_closure_captures::collect(ast, c.value, &mut inner);
        for s in &c.body {
            collect_stmt(ast, s, &mut inner);
        }
        lexical_names(&c.body, &mut bound);
    }
    if let Some(d) = default {
        for s in d {
            collect_stmt(ast, s, &mut inner);
        }
        lexical_names(d, &mut bound);
    }
    drain_unbound(inner, &bound, out);
}

/// A statement list that opens a scope of its own: collect, then drop
/// what it declares.
fn region<'a>(ast: &'a Ast, list: &'a [Stmt], extra: &[&'a str], out: &mut Vec<&'a str>) {
    let mut inner: Vec<&'a str> = Vec::new();
    for s in list {
        collect_stmt(ast, s, &mut inner);
    }
    let mut bound: Vec<&'a str> = extra.to_vec();
    lexical_names(list, &mut bound);
    drain_unbound(inner, &bound, out);
}

fn drain_unbound<'a>(inner: Vec<&'a str>, bound: &[&'a str], out: &mut Vec<&'a str>) {
    out.extend(inner.into_iter().filter(|c| !bound.contains(c)));
}

/// The names a list declares directly. `Stmt::Multi` is a transparent
/// grouping, so its members declare into this list.
fn lexical_names<'a>(list: &'a [Stmt], bound: &mut Vec<&'a str>) {
    for s in list {
        match s {
            Stmt::LetDecl { name, .. }
            | Stmt::UsingDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::ClassDecl { name, .. } => bound.push(name.as_str()),
            Stmt::YieldInto { var, .. } => bound.push(var.as_str()),
            Stmt::Multi(inner) => lexical_names(inner, bound),
            _ => {}
        }
    }
}
