//! Calls to builtin values the spec makes uncallable — resolved to
//! their runtime TypeError at the site (goal-triage family fifth
//! member, next door to the readonly-write triage).
//!
//! The checker refuses `Map()` / `JSON()` as "not callable", which
//! stops the whole compile — but §13.3.6.2 EvaluateCall makes the
//! failure a RUNTIME event: the arguments evaluate first (step 4),
//! then the IsCallable check throws a TypeError (step 6). Unlike the
//! readonly-write siblings there is no goal split — calling an
//! uncallable value throws under both goals — so every site rewrites
//! to a throw the same way.
//!
//! The target set is closed and statically decidable: the namespace
//! objects that have no [[Call]] at all, plus the constructors whose
//! spec first step is "If NewTarget is undefined, throw a TypeError".
//! Callable builtins (`Object` / `String` / `Date` / `Function` …)
//! stay out — their call-without-new forms have real semantics. A
//! program declaration shadowing the name owns it (the same
//! conservative whole-program declared-set the family shares; a
//! local-only shadow over-exempts, which fails safe: the checker
//! still sees the original site).
//!
//! The rewrite happens on the arena slot, so any position — top
//! level, function body, value position — is covered without a tree
//! walk. Arguments are sequenced ahead of the throw via the comma
//! operator so their side effects still happen in source order.
//!
//! Runs with the readonly siblings: after the eval inline, before
//! the checker.

use super::{Ast, Expr, ExprId, Stmt};

/// Namespace objects with no [[Call]] — §21.3 Math, §25.5 JSON,
/// §28.1 Reflect, §25.4 Atomics. Plain objects; calling one is a
/// TypeError under every goal.
const UNCALLABLE_NAMESPACES: &[&str] = &["Math", "JSON", "Reflect", "Atomics"];

/// Constructors whose spec first step is "If NewTarget is undefined,
/// throw a TypeError" — callable only through `new`. `Iterator`
/// additionally throws even WITH `new` when constructed directly
/// (§27.1.3.1), so its call form is a fortiori a throw.
const NEW_ONLY_CTORS: &[&str] = &[
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "WeakRef",
    "FinalizationRegistry",
    "Promise",
    "Proxy",
    "ArrayBuffer",
    "SharedArrayBuffer",
    "DataView",
    "Iterator",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

/// What a resolved site becomes.
enum SiteKind {
    /// The §13.3.6.2 step-6 TypeError, with this message.
    Throw(String),
    /// §21.4.2.1 `Date(...)` without `new` — NOT uncallable, but the
    /// one builtin whose call form ignores its arguments entirely:
    /// the arguments evaluate (step 4) and are then discarded, and
    /// the call returns the current time as a String, exactly
    /// `new Date().toString()`. No ToPrimitive ever runs on them.
    DateString,
}

pub fn resolve_uncallable_builtin_calls(ast: &mut Ast) {
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    let sites: Vec<(usize, Vec<ExprId>, SiteKind)> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let Expr::Call { callee, args } = e else {
                return None;
            };
            let Expr::Ident(n) = ast.get_expr(*callee) else {
                return None;
            };
            if declared.contains(n) {
                return None;
            }
            let kind = if UNCALLABLE_NAMESPACES.contains(&n.as_str()) {
                SiteKind::Throw(format!("{n} is not a function"))
            } else if NEW_ONLY_CTORS.contains(&n.as_str()) {
                SiteKind::Throw(format!("calling {n} constructor without new is invalid"))
            } else if n == "Date" {
                SiteKind::DateString
            } else {
                return None;
            };
            Some((i, args.clone(), kind))
        })
        .collect();
    for (i, args, kind) in sites {
        // §13.3.6.2 — ArgumentListEvaluation (step 4) runs before
        // the IsCallable check (step 6): sequence each argument
        // ahead of the payload so its side effects happen in order.
        // The nested nodes are freshly added; the old Call slot is
        // overwritten with the outermost, whose fresh slot goes
        // orphaned (nothing references it).
        let payload = match kind {
            SiteKind::Throw(msg) => {
                let msg_e = ast.add_expr(Expr::String(msg.into()));
                let exc = ast.add_expr(Expr::New {
                    class_name: "TypeError".to_string(),
                    args: vec![msg_e],
                    type_args: Vec::new(),
                });
                let arrow = ast.add_expr(Expr::ArrowFn {
                    params: Vec::new(),
                    return_type: None,
                    body: vec![Stmt::Throw(exc)],
                });
                ast.add_expr(Expr::Call {
                    callee: arrow,
                    args: Vec::new(),
                })
            }
            SiteKind::DateString => {
                let date = ast.add_expr(Expr::New {
                    class_name: "Date".to_string(),
                    args: Vec::new(),
                    type_args: Vec::new(),
                });
                let to_string = ast.add_expr(Expr::Member {
                    obj: date,
                    name: "toString".to_string(),
                });
                ast.add_expr(Expr::Call {
                    callee: to_string,
                    args: Vec::new(),
                })
            }
        };
        let mut site = payload;
        for a in args.into_iter().rev() {
            site = ast.add_expr(Expr::Sequence {
                left: a,
                right: site,
            });
        }
        ast.exprs[i] = ast.get_expr(site).clone();
    }
}
