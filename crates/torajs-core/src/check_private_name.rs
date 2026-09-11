//! Is a resolved private-name reference one the class actually
//! declares?
//!
//! `resolve_private_refs` rewrites every `#x` site to
//! `__priv_<C>__<x>` where `<C>` is the innermost enclosing class that
//! declares `x` — and, when NOTHING declares it, to the innermost
//! scope anyway, so that the checker can turn §13.1's "unresolvable
//! private name" into a compile-time reject. The two spellings are
//! identical by the time they arrive, which is why the reject sites
//! used "the instance member list has no such field" as their proxy
//! for "undeclared".
//!
//! That proxy is wrong for a STATIC private. `class C { static #f() {}
//! g() { return this.#f() } }` is legal code — §15.7 puts every
//! PrivateName of the body in scope for the whole body, and the
//! mismatch between the reference and the instance is a RUNTIME brand
//! check (§7.3.32 PrivateElementFind → TypeError), the very thing the
//! test262 `static-private-method-referenced-from-instance-method`
//! family asserts. Asking the scope table directly separates the two
//! readings: declared anywhere in the class means the reference is
//! legal and the runtime decides; declared nowhere is the early error.

use crate::ast::Ast;

/// The raw private name behind a mangled member name, for a
/// diagnostic that wants to spell it `#x` the way the source did.
pub(crate) fn raw_private_name(name: &str) -> &str {
    let rest = name.strip_prefix("__priv_").unwrap_or(name);
    // The class name may itself contain `__` (a synthetic
    // `__ClassExpr_0`), so the raw name is what follows the LAST
    // separator.
    rest.rfind("__").map_or(rest, |at| &rest[at + 2..])
}

/// Every `__priv_<C>__<raw>` some class body declares — a field, a
/// method or an accessor, static or not. Taken once per program: the
/// reject sites only ever ask whether a resolved name is in here.
///
/// A parse-time class name can repeat across a file, so two scopes
/// named `C` contribute to one set. The over-admit hands the question
/// to the runtime brand check, which is where §7.3.32 answers it.
pub(crate) fn declared_private_names(ast: &Ast) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for (cls, names) in &ast.class_private_scopes {
        for raw in names {
            out.insert(format!("__priv_{cls}__{raw}"));
        }
    }
    out
}
