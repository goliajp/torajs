//! Syntactic array evidence for the spread expanders (rotation 626).
//!
//! Every spread walk in this family — `apply_spread_args`'s
//! fixed-arity expansion, `xs.push(...src)`, the `Math.min/max`
//! reducer — reads its Ident source by INDEX, which is the spread's
//! meaning for an array and for nothing else. A generator object, a
//! Set, a string: indexed, they answer undefined (or a code unit),
//! and the §7.4 iteration the spread prescribes never happens —
//! `f(...gen)` on `function f(a, b)` read two undefineds and the
//! generator body never ran, the same hole rotation 625 closed for a
//! CALL source by hoisting `[...mk()]`.
//!
//! The walks run before the checker, so the only evidence is
//! syntactic: a binding whose declaration says "array" — a `T[]` /
//! `Array<T>` annotation, an array-literal initializer (the hoist's
//! own `__sps_N = [...src]` temps included), or a rest parameter. A
//! name bound anywhere without such evidence is not evident (the
//! walks are name-keyed, so one doubtful binding taints the name).
//! A source that is not evident is hoisted into `[...src]` — one
//! copy, one §7.4 iteration — or, where the hoist abstains, left to
//! the runtime spread lane (`ssa_lower_call_spread`), which iterates.
//! An evident array keeps the zero-copy index expansion.

use super::ns_alias::walk_stmts;
use super::{Ast, Expr, Stmt};
use std::collections::HashSet;

/// The names whose every binding is syntactically an array.
pub(super) fn collect_array_evident(ast: &Ast) -> HashSet<String> {
    let mut evident = HashSet::new();
    let mut tainted = HashSet::new();
    walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let is_array = type_ann.as_deref().is_some_and(ann_is_array)
                || matches!(ast.get_expr(*init), Expr::Array(_));
            note(&mut evident, &mut tainted, name, is_array);
        }
        Stmt::FnDecl { params, .. } => {
            for p in params {
                let is_array = p.is_rest || p.type_ann.as_deref().is_some_and(ann_is_array);
                note(&mut evident, &mut tainted, &p.name, is_array);
            }
        }
        _ => {}
    });
    evident.retain(|n| !tainted.contains(n));
    // The arguments object itself: its `@@iterator` is
    // `%Array.prototype.values%` (§10.4.4.6/7 step 21), so reading it
    // by index IS its iteration — and the static faces leave a
    // `...arguments` spread for the expander to index-expand (the
    // rewrite to params / `__torajs_arguments` follows). A user
    // binding of that name (the sloppy shadow) is an ordinary local
    // and taints it like any other.
    if !tainted.contains("arguments") {
        evident.insert("arguments".to_string());
    }
    evident
}

fn note(evident: &mut HashSet<String>, tainted: &mut HashSet<String>, name: &str, is_array: bool) {
    if is_array {
        evident.insert(name.to_string());
    } else {
        tainted.insert(name.to_string());
    }
}

/// `T[]`, `readonly T[]`, `Array<T>`, `ReadonlyArray<T>` — the
/// spellings that name an array. A union (`A | B[]`) is not one; a
/// parenthesized element union (`(A | B)[]`) is, but is passed over
/// too — the cost of passing over evidence is one copy, the cost of
/// inventing it is an indexed non-array.
fn ann_is_array(ann: &str) -> bool {
    let t = ann.trim();
    let t = t.strip_prefix("readonly ").unwrap_or(t).trim_start();
    (t.ends_with("[]") && !t.contains('|'))
        || t.starts_with("Array<")
        || t.starts_with("ReadonlyArray<")
}
