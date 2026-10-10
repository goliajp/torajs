//! Whether a program can re-parent an existing object at all (RFC
//! 20260920-struct-user-proto K4).
//!
//! A class instance's typed lane answers its methods, accessors and
//! `instanceof` from its class statically. Once an instance can take a
//! user [[Prototype]] that is no longer sound, but only in a program
//! that can reach a re-parenting write — every other program keeps the
//! static lane at zero cost. Aliasing makes the question undecidable
//! per class (`const a: any = c; Object.setPrototypeOf(a, p)`), so it
//! is asked once per program, over the user's own code: it runs before
//! any injection, and the injected helpers are the compiler's.
//!
//! The spellings that reach a re-parent: `setPrototypeOf` and
//! `__proto__` (member, optional member, or a string used as a key),
//! `construct` (`Reflect.construct`'s newTarget), and `Proxy` (a `get`
//! trap answers a constructor's `prototype`). Every expression in the
//! arena is looked at, reachable or not, so a hit is conservative.

use super::{Ast, Expr};

/// Record on `ast` whether its user code names a re-parenting write,
/// and the census that reads it (`named_prop_census`).
pub fn census_program_reparents(ast: &mut Ast) {
    ast.program_reparents = ast.exprs.iter().any(names_a_reparent);
    super::named_prop_census::census_array_prop_reads(ast);
}

fn names_a_reparent(e: &Expr) -> bool {
    match e {
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => reparent_key(name),
        Expr::String(s) => ["setPrototypeOf", "__proto__", "construct"]
            .iter()
            .any(|k| s == *k),
        Expr::Ident(name) => name == "Proxy",
        _ => false,
    }
}

fn reparent_key(name: &str) -> bool {
    matches!(name, "setPrototypeOf" | "__proto__" | "construct")
}
