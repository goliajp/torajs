//! Forward references between closure bindings in one statement list —
//! two arrows that call each other:
//!
//! ```text
//! const isEven = (n) => n === 0 ? true  : isOdd(n - 1);
//! const isOdd  = (n) => n === 0 ? false : isEven(n - 1);
//! ```
//!
//! `isEven`'s capture list names `isOdd`, which is not declared until
//! the next statement, so resolving captures against the scope as it
//! stands answers "unknown identifier". ES hoists `let` / `const` to the
//! top of their block (in TDZ until the declaration runs), and both
//! bodies only execute after both declarations have, so nothing here is
//! actually being read early.
//!
//! [`crate::check_stmt_let_decl`] handles the SELF-reference case one
//! statement at a time and declares into the scope proper. This pass
//! covers what a single statement cannot see — a peer declared LATER —
//! and deliberately keeps its answers in a side table rather than in
//! `scopes`, so the real declare each binding eventually performs stays
//! an ordinary first declaration and a genuine redeclaration still
//! reports as one.
//!
//! Narrow twice over: only bindings whose initializer IS a closure, and
//! only those some closure in the same list actually captures.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, Stmt};
use crate::check::{Checker, Type};

/// Fill the checker's hoist table from `stmts` and answer the previous
/// contents — the caller restores them when the list is done, so a name
/// hoisted for one block cannot answer a capture in an unrelated one.
pub(crate) fn enter(checker: &mut Checker, ast: &Ast, stmts: &[Stmt]) -> HashMap<String, Type> {
    let mut decls: Vec<(&str, &str, &Option<String>)> = Vec::new();
    let mut captured: HashSet<&str> = HashSet::new();
    collect(ast, stmts, &mut decls, &mut captured);
    let saved = std::mem::take(&mut checker.hoisted_closure_lets);
    if decls.is_empty() {
        return saved;
    }
    checker.hoisted_closure_lets = saved.clone();
    for (name, fn_name, type_ann) in decls {
        if !captured.contains(name) {
            continue;
        }
        // Same gate as the self-reference pre-declare: an `any`-annotated
        // binding does not take a closure slot, so admitting it here
        // would only move an honest type error to lower time.
        if let Some(ann) = type_ann
            && !matches!(
                crate::check_type_ann::resolve_type_ann_full(
                    ann,
                    &checker.aliases,
                    &[],
                    &checker.generic_alias_decls,
                ),
                Some(Type::Function(..))
            )
        {
            continue;
        }
        if let Ok(sig) = crate::check_type_of_fn::closure_sig(ast, fn_name, &checker.aliases) {
            checker
                .hoisted_closure_lets
                .insert(name.to_string(), sig.value_ty);
        }
    }
    saved
}

/// The list's closure-initialized bindings, and every name any of their
/// closures captures. `Stmt::Multi` is a transparent grouping that
/// shares the surrounding scope, so it flattens in; anything that opens
/// a scope of its own does not — that block runs this pass itself.
fn collect<'a>(
    ast: &'a Ast,
    stmts: &'a [Stmt],
    decls: &mut Vec<(&'a str, &'a str, &'a Option<String>)>,
    captured: &mut HashSet<&'a str>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } => {
                if let Expr::Closure { fn_name, captures } = ast.get_expr(*init) {
                    decls.push((name.as_str(), fn_name.as_str(), type_ann));
                    for c in captures {
                        captured.insert(c.as_str());
                    }
                }
            }
            Stmt::Multi(inner) => collect(ast, inner, decls, captured),
            _ => {}
        }
    }
}
