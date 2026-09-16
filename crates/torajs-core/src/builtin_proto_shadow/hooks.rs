//! The two whole-program scans that answer "what did this module
//! install behind the typed tier's back" — the well-known symbol
//! handlers and the namespace statics.
//!
//! A sibling of [`super::scan`] rather than part of it: that file
//! answers which builtin PROTOTYPE members a module can reach, and
//! these two answer about two other objects entirely — the symbol
//! keys any value can carry, and the constructor's own members.
//! Splitting them apart is also what keeps `scan.rs` inside the
//! file-size limit.

use super::{ShadowSet, scan::str_literal};
use crate::ast::{Ast, Expr, ExprId};

/// The objects whose STATIC members the typed tier claims off the
/// callee's syntax: the constructors plus the three pure namespaces.
/// A write to one of their members is invisible to every such arm.
///
/// `Promise` is deliberately absent: its statics already have a
/// mechanism for this (rotation 448 — the combinators invoke a
/// patched `Promise.resolve` per element while the async driver's own
/// settle operations bypass it), and routing those call sites to the
/// any lane instead loses it — the two gate fixtures that cover it
/// went to a wrong answer and a SIGSEGV.
const STATIC_NAMESPACES: &[&str] = &[
    "Object", "Array", "String", "Number", "Boolean", "Date", "RegExp", "Map", "Set", "WeakMap",
    "WeakSet", "Function", "BigInt", "Symbol", "JSON", "Math", "Reflect",
];

/// Which `<Namespace>.<member>` pairs does this module write? Two
/// spellings name one: a member assignment (`Object.keys = f`) and a
/// `defineProperty` whose receiver is the namespace. A computed key
/// (`Math[k] = f`) names no member the scan can read, so the whole
/// namespace is recorded opaque.
///
/// A READ is not a write, and neither is a call: `Object.keys(o)` is
/// the very thing the answer protects.
pub(super) fn scan_namespace_statics(ast: &Ast, set: &mut ShadowSet) {
    // `as` layers are value pass-throughs — `(Object as any).keys = f`
    // writes the same property `Object.keys = f` does.
    let strip_as = |mut e: ExprId| {
        while let Expr::As { expr, .. } = ast.get_expr(e) {
            e = *expr;
        }
        e
    };
    let ns_of = |e: ExprId| match ast.get_expr(strip_as(e)) {
        Expr::Ident(n) => STATIC_NAMESPACES.iter().copied().find(|c| *c == n.as_str()),
        _ => None,
    };
    // A REMOVAL is a write too, and a louder one: `delete Object.keys`
    // makes `Object.keys(o)` a TypeError, and a route that folds the
    // call off the callee's syntax answers it anyway.
    let touch = |target: ExprId, set: &mut ShadowSet| match ast.get_expr(target) {
        Expr::Member { obj, name } => {
            if let Some(ns) = ns_of(*obj) {
                set.add_static_write(ns, name);
            }
        }
        Expr::Index { obj, index } => {
            if let Some(ns) = ns_of(*obj) {
                match str_literal(ast, *index) {
                    Some(k) => set.add_static_write(ns, &k),
                    None => set.add_static_opaque(ns),
                }
            }
        }
        _ => {}
    };
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } => touch(*target, set),
            Expr::Delete { expr } => touch(*expr, set),
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    continue;
                };
                if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Object" || n == "Reflect")
                {
                    continue;
                }
                if !matches!(name.as_str(), "defineProperty" | "defineProperties") {
                    continue;
                }
                let Some(ns) = args.first().copied().and_then(ns_of) else {
                    continue;
                };
                match args.get(1).copied().and_then(|k| str_literal(ast, k)) {
                    Some(k) if name == "defineProperty" => set.add_static_write(ns, &k),
                    _ => set.add_static_opaque(ns),
                }
            }
            _ => {}
        }
    }
}

pub(super) fn scan_symbol_hooks(ast: &Ast, set: &mut ShadowSet) {
    let symbol_prop = |e: ExprId| match ast.get_expr(e) {
        Expr::Member { obj, name } if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Symbol") => {
            Some(name.as_str())
        }
        _ => None,
    };
    // A REMOVAL is a write: `delete Array.prototype[Symbol.iterator]`
    // makes `[...arr]` a TypeError, and the typed lanes that read the
    // backing store answer the elements instead.
    let touch = |target: ExprId, set: &mut ShadowSet| {
        if let Expr::Index { index, .. } = ast.get_expr(target)
            && let Some(p) = symbol_prop(*index)
        {
            set.add_symbol_hook(p);
        }
    };
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } => touch(*target, set),
            Expr::Delete { expr } => touch(*expr, set),
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                    continue;
                };
                if !matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Object" || n == "Reflect")
                {
                    continue;
                }
                match name.as_str() {
                    "defineProperty" => {
                        if let Some(&k) = args.get(1)
                            && let Some(p) = symbol_prop(k)
                        {
                            set.add_symbol_hook(p);
                        }
                    }
                    "defineProperties" => {
                        if args.get(1).is_some_and(|&d| {
                            matches!(ast.get_expr(d), Expr::ObjectLit { fields }
                                if fields.iter().any(|(n, _)| n.starts_with("__computed_")))
                        }) {
                            set.set_symbol_hook_opaque();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
