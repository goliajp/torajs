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
//! The captured binding need not be a closure itself:
//!
//! ```text
//! const g = () => o.v;
//! const o = { v: 3 };
//! ```
//!
//! is the same shape with the same answer — both declarations run
//! before anything calls `g`. Such a binding takes its type from its
//! annotation, or from its initializer when it has none.
//!
//! Narrow either way: only bindings some closure in the same list
//! actually captures, and for the ordinary ones only when that closure
//! was declared EARLIER (a later one finds the binding in scope on its
//! own, and hoisting it would answer a question nobody asked).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, Stmt};
use crate::check::{Checker, Type};

/// Fill the checker's hoist table from `stmts` and answer the previous
/// contents — the caller restores them when the list is done, so a name
/// hoisted for one block cannot answer a capture in an unrelated one.
pub(crate) fn enter(checker: &mut Checker, ast: &Ast, stmts: &[Stmt]) -> HashMap<String, Type> {
    let mut decls: Vec<(&str, &str, &Option<String>)> = Vec::new();
    let mut plain: Vec<(&str, &Option<String>, crate::ast::ExprId)> = Vec::new();
    let mut captured: HashSet<&str> = HashSet::new();
    collect(ast, stmts, &mut decls, &mut plain, &mut captured);
    let saved = std::mem::take(&mut checker.hoisted_closure_lets);
    if decls.is_empty() && plain.is_empty() {
        return saved;
    }
    checker.hoisted_closure_lets = saved.clone();
    for (name, type_ann, init) in plain {
        // An annotation is the binding's own answer; without one the
        // init's type is it. Typing that expression early can fail
        // when it references the list's OWN earlier declarations
        // (`let iterable = {...}; let iterator = f(iterable)` — at
        // hoist time nothing in the list is in scope yet), so a
        // failure hoists `Any` instead of giving up: the capture
        // resolves, and a genuinely-bad init still errors when the
        // declaration statement itself re-types it. `type_of` only
        // caches into `expr_types`, which the ordinary pass over
        // this statement overwrites with the same answer.
        let ty = match type_ann {
            Some(ann) => crate::check_type_ann::resolve_type_ann_full(
                ann,
                &checker.aliases,
                &[],
                &checker.generic_alias_decls,
            ),
            None => checker.type_of(ast, init).ok(),
        };
        checker
            .hoisted_closure_lets
            .insert(name.to_string(), ty.unwrap_or(Type::Any));
    }
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
    plain: &mut Vec<(&'a str, &'a Option<String>, crate::ast::ExprId)>,
    captured: &mut HashSet<&'a str>,
) {
    // Ordinary bindings need the captures seen SO FAR, not the list's
    // union: one captured only by a closure declared after it is
    // already in scope by then and must stay an ordinary declaration.
    // The closure-initialized ones keep the union they have always
    // used — self-reference is the per-statement pass's case and does
    // not depend on this ordering.
    let mut seen: HashSet<&'a str> = HashSet::new();
    collect_inner(ast, stmts, decls, plain, captured, &mut seen);
}

fn collect_inner<'a>(
    ast: &'a Ast,
    stmts: &'a [Stmt],
    decls: &mut Vec<(&'a str, &'a str, &'a Option<String>)>,
    plain: &mut Vec<(&'a str, &'a Option<String>, crate::ast::ExprId)>,
    captured: &mut HashSet<&'a str>,
    seen: &mut HashSet<&'a str>,
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
                        seen.insert(c.as_str());
                    }
                } else {
                    if seen.contains(name.as_str()) {
                        plain.push((name.as_str(), type_ann, *init));
                    }
                    // A closure minted DEEPER in the init (object-
                    // literal method, array element, `new` argument)
                    // forward-references the same way a closure init
                    // does — its captures mark later bindings for the
                    // plain hoist. Extra `seen` entries are harmless:
                    // they only matter when a later binding matches
                    // one, and matching means it was captured.
                    let mut nested: Vec<&str> = Vec::new();
                    crate::ast::nested_closure_captures::collect(ast, *init, &mut nested);
                    for c in nested {
                        captured.insert(c);
                        seen.insert(c);
                    }
                }
            }
            Stmt::Multi(inner) => collect_inner(ast, inner, decls, plain, captured, seen),
            // Every other statement position mints closures too — an
            // assignment, a call argument, a condition, a nested block
            // of this same region. §8.2.6 creates the block's lexical
            // bindings on entry, so one of those closures naming a
            // binding declared LATER in this list means THAT binding,
            // which is only resolvable if it hoists.
            other => {
                let mut caps: Vec<&str> = Vec::new();
                crate::ast::stmt_closure_captures::collect_stmt(ast, other, &mut caps);
                for c in caps {
                    captured.insert(c);
                    seen.insert(c);
                }
            }
        }
    }
}
