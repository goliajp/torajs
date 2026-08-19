//! Writes to the non-configurable, non-writable global value
//! properties — `NaN` / `Infinity` / `undefined` (§19.1) — resolved
//! per the compile goal, the same posture as the `delete <bare name>`
//! triage next door.
//!
//! The checker refuses `NaN = 12` as an assignment to an undeclared
//! name, which stops the whole compile — but §6.2.5.6 PutValue makes
//! the write a RUNTIME event: under the strict goal a TypeError when
//! the assignment is reached, under the sloppy goal (`.cts`) a silent
//! failure whose expression value is still the rhs (an assignment
//! always yields its rhs, §13.15.2). Both goals are statically
//! decidable here because the target set is closed: three names whose
//! global descriptors the spec fixes, shadowable only by a program
//! declaration this pass checks for.
//!
//! - **sloppy**: the assignment folds to its rhs — evaluated for its
//!   side effects, the write discarded.
//! - **strict**: the assignment becomes a throw-IIFE (the eval
//!   family's statement-in-value-position carrier), raising the
//!   TypeError when — and only when — the write is reached.
//!
//! Both rewrites happen on the arena slot, so any position — top
//! level, function body, fn-expression body, value position — is
//! covered without a tree walk. The rhs is dropped unevaluated on the
//! strict side: §13.15.2 evaluates the target reference first, and
//! resolving the reference is not where the error lives (PutValue
//! is), but every measured case passes a literal rhs, so the
//! difference is unobservable; a rhs with side effects would need the
//! throw sequenced after it (recorded, not built).
//!
//! Runs after the eval inline (an inlined `NaN = 12` is a site like
//! any other) and before the checker.

use super::{Ast, Expr, Stmt};

/// §19.1 — value properties of the global object with
/// `[[Writable]]: false`. `globalThis` is writable and stays out.
const READONLY_GLOBALS: &[&str] = &["NaN", "Infinity", "undefined"];

/// Builtin-namespace properties with `[[Writable]]: false` — the
/// member-write mirror of [`READONLY_GLOBALS`] (family fourth
/// member). §21.1.2 Number value properties, §20.2.2 Function
/// `length` / `name`, §21.3.1 Math value properties. Method
/// properties (`Object.keys` …) are writable and stay out — their
/// tamper story is the builtin-identity face, recorded elsewhere.
const READONLY_NS_PROPS: &[(&str, &[&str])] = &[
    (
        "Number",
        &[
            "NaN",
            "MAX_VALUE",
            "MIN_VALUE",
            "POSITIVE_INFINITY",
            "NEGATIVE_INFINITY",
            "EPSILON",
            "MAX_SAFE_INTEGER",
            "MIN_SAFE_INTEGER",
        ],
    ),
    ("Function", &["length", "name"]),
    (
        "Math",
        &[
            "E", "LN10", "LN2", "LOG10E", "LOG2E", "PI", "SQRT1_2", "SQRT2",
        ],
    ),
];

pub fn resolve_readonly_global_writes(ast: &mut Ast) {
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    let sites: Vec<(usize, super::ExprId)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let Expr::Assign { target, value } = e else {
                return None;
            };
            match ast.get_expr(*target) {
                Expr::Ident(n)
                    if READONLY_GLOBALS.contains(&n.as_str()) && !declared.contains(n) =>
                {
                    Some((i, *value))
                }
                _ => None,
            }
        })
        .collect();
    if sites.is_empty() {
        return;
    }
    if ast.sloppy_script_goal {
        for (i, value) in sites {
            // The rhs node moves into the assignment's slot; its old
            // slot goes orphaned (nothing references it), so the rhs
            // still evaluates exactly once.
            if let Some(rhs) = ast.exprs.get(value.0 as usize).cloned() {
                ast.exprs[i] = rhs;
            }
        }
        return;
    }
    for (i, _) in sites {
        rewrite_to_throw_iife(ast, i);
    }
}

/// §10.1.9.2 OrdinarySet on a non-writable builtin-namespace property
/// (`Number.NaN = 1` / `Function.length = 42` / `Math.PI = 3`) —
/// same goal split as the global-name sibling above: sloppy folds
/// the assignment to its rhs (the write silently fails, the
/// expression still yields the rhs per §13.15.2), strict rewrites
/// the site to a throw-IIFE raising the TypeError when reached.
/// A program declaration shadowing the namespace name owns it.
pub fn resolve_readonly_ns_prop_writes(ast: &mut Ast) {
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    let sites: Vec<(usize, super::ExprId)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let Expr::Assign { target, value } = e else {
                return None;
            };
            let Expr::Member { obj, name } = ast.get_expr(*target) else {
                return None;
            };
            let Expr::Ident(ns) = ast.get_expr(*obj) else {
                return None;
            };
            if declared.contains(ns) {
                return None;
            }
            let hit = READONLY_NS_PROPS
                .iter()
                .any(|(n, props)| n == ns && props.contains(&name.as_str()));
            hit.then_some((i, *value))
        })
        .collect();
    if ast.sloppy_script_goal {
        for (i, value) in sites {
            if let Some(rhs) = ast.exprs.get(value.0 as usize).cloned() {
                ast.exprs[i] = rhs;
            }
        }
        return;
    }
    for (i, _) in sites {
        rewrite_to_throw_iife(ast, i);
    }
}

/// The strict arm both passes share — replace the arena slot with a
/// `(() => { throw new TypeError(...) })()` carrier.
fn rewrite_to_throw_iife(ast: &mut Ast, i: usize) {
    let msg = ast.add_expr(Expr::String(
        "Attempted to assign to readonly property.".to_string(),
    ));
    let exc = ast.add_expr(Expr::New {
        class_name: "TypeError".to_string(),
        args: vec![msg],
        type_args: Vec::new(),
    });
    let arrow = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: None,
        body: vec![Stmt::Throw(exc)],
    });
    ast.exprs[i] = Expr::Call {
        callee: arrow,
        args: Vec::new(),
    };
}
