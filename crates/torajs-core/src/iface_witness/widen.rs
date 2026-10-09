//! A class field the interface writes a wider value into.
//!
//! `interface P { x: number | string }` lets `class C { x = 1 }` in:
//! tsc compares properties covariantly, so `const p: P = c; p.x = "s"`
//! is a legal program, and afterwards `c.x` is `"s"` as much as `p.x`
//! is. A `number` slot cannot hold that string, and a store that kept
//! it beside the slot would leave every typed read of `c.x` on the old
//! number. So when such a member is written through any receiver but
//! the instance's own `this` — the write may be one through the
//! interface, which nothing before the check can rule out — the field
//! is held as `any` in its class and in every class sharing its slot,
//! and every read and write of it, through the class or the
//! interface, reaches the one value it holds.

use std::collections::BTreeSet;

use super::plan::IfacePlan;
use crate::ast::{Ast, Expr, Stmt};

/// Hold as `any` every retyped field of `plans` the program writes
/// through a receiver other than `this`; true when one changed.
pub(super) fn widen_written(ast: &mut Ast, plans: &[IfacePlan]) -> bool {
    let mut want: BTreeSet<(String, String)> = BTreeSet::new();
    let mut written: Option<BTreeSet<String>> = None;
    for p in plans {
        for (c, keys) in &p.retyped {
            let w = written.get_or_insert_with(|| written_keys(ast));
            for k in keys.iter().filter(|k| w.contains(*k)) {
                want.insert((declarer(ast, c, k), k.clone()));
            }
        }
    }
    let mut changed = false;
    for (root, key) in &want {
        for c in sharing(ast, root) {
            changed |= hold_any(&mut ast.stmts, &c, key);
        }
    }
    changed
}

/// The member names some assignment or update writes through a
/// receiver other than the instance's own `this`.
fn written_keys(ast: &Ast) -> BTreeSet<String> {
    let own = |obj: crate::ast::ExprId| match ast.get_expr(obj) {
        Expr::This => true,
        Expr::Ident(n) => n == "this" || n == "__this",
        _ => false,
    };
    let key = |target: crate::ast::ExprId| match ast.get_expr(target) {
        Expr::Member { obj, name } if !own(*obj) => Some(name.clone()),
        Expr::Index { obj, index } if !own(*obj) => match ast.get_expr(*index) {
            Expr::String(s) => s.as_str().map(str::to_string),
            _ => None,
        },
        _ => None,
    };
    (ast.exprs.iter())
        .filter_map(|e| match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => key(*target),
            _ => None,
        })
        .collect()
}

/// The topmost ancestor of `c` (or `c` itself) declaring field `key`:
/// the class whose slot every descendant shares.
fn declarer(ast: &Ast, c: &str, key: &str) -> String {
    let mut at = c.to_string();
    while let Some(Some(p)) = ast.class_parents.get(&at)
        && has_field(ast, p, key)
    {
        at = p.clone();
    }
    at
}

/// `root` and every class below it.
fn sharing(ast: &Ast, root: &str) -> Vec<String> {
    let below = |c: &str| {
        let mut at = Some(c.to_string());
        while let Some(x) = at {
            if x == root {
                return true;
            }
            at = ast.class_parents.get(&x).cloned().flatten();
        }
        false
    };
    (ast.class_parents.keys())
        .filter(|c| below(c))
        .cloned()
        .collect()
}

fn has_field(ast: &Ast, c: &str, key: &str) -> bool {
    crate::ast::toplevel_stmts_flat(ast).into_iter().any(|s| {
        matches!(s, Stmt::TypeDecl { name, fields, .. }
            if name == c && fields.iter().any(|(n, _)| *n == *key))
    })
}

/// Annotate field `key` of class `c` `any`; true when it was not.
fn hold_any(stmts: &mut [Stmt], c: &str, key: &str) -> bool {
    let mut changed = false;
    for s in stmts {
        match s {
            Stmt::Multi(inner) => changed |= hold_any(inner, c, key),
            Stmt::TypeDecl { name, fields, .. } if name == c => {
                for (n, ann) in fields.iter_mut() {
                    if *n == *key && ann.trim() != "any" {
                        *ann = "any".to_string();
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }
    changed
}
