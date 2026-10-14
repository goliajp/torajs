//! Whether a read of an array's non-index property can run user code.
//!
//! `xs[1.5]` on a typed array names the property "1.5" (§7.1.19). The
//! runtime answers it from the keyed read when the cell can carry a
//! named property — an own expando, an Array subclass, a user property
//! on %Array.prototype% or %Object.prototype% — and that read raises
//! only through user code: a getter (§10.1.8.1 step 7), or the
//! ToNumber / ToString the typed lane applies to a value of another
//! type (a `valueOf` that throws). This census covers the prototype
//! and accessor half: a program naming none of the spellings below
//! cannot put such a property on an array's prototype chain nor
//! define an accessor anywhere. The other half, an own expando
//! holding a value of another type, needs a keyed write through a
//! receiver whose type does not fix the element, and the throw
//! analysis finds those from the checker's types
//! (`Scan::code_write`). Both answers are needed before a
//! non-integral read may stay off its callers' checks. A string's read
//! at a key it does not own (`s[1.5]`, `s[7]` past the end) relies on
//! the same answer for %String.prototype%, which the spellings below
//! reach the way they reach %Array.prototype% (`String.prototype`,
//! `getPrototypeOf`, `constructor`).
//!
//! The spellings: a re-parent (`program_reparents`); an accessor
//! definition or a bulk copy (`defineProperty`, `defineProperties`,
//! `__defineGetter__`, `assign`); a way to hold a builtin prototype
//! or constructor (`getPrototypeOf`, `constructor`, `prototype` off
//! anything but a function or class this program declares, a member
//! named `Array` / `Object` / `Reflect`, so `globalThis.Array` before
//! its rewrite); and `Object`, `Reflect`, `Array` or `globalThis` used
//! as a value, which covers `extends Array`, an alias and a computed
//! key alike. Like the re-parent census it runs over the user's code
//! before injection, and over every expression in the arena,
//! reachable or not.

pub(super) mod accessor_names;
mod cast_targets;
pub(super) mod shadow_names;
mod write_kind;

use super::{Ast, Expr, ExprId, Stmt};
use std::collections::HashSet;

/// Record on `ast` whether its user code can put a code-bearing named
/// property on an array's prototype chain. Reads `program_reparents`,
/// so it runs after that census.
pub(super) fn census_array_prop_reads(ast: &mut Ast) {
    let named_bases = named_bases(ast);
    let own_ctors = own_ctors(ast);
    ast.array_prop_reads_may_raise = ast.program_reparents
        || ast.exprs.iter().enumerate().any(|(i, e)| match e {
            Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
                REACHING_KEYS.contains(&name.as_str())
                    || holds_a_chain(name)
                    || (name == "prototype"
                        && !matches!(ast.get_expr(*obj),
                            Expr::Ident(n) if own_ctors.contains(n)))
            }
            Expr::String(s) => REACHING_KEYS.iter().any(|k| s == *k) || s == "prototype",
            Expr::Index { index, .. } | Expr::OptIndex { index, .. } => {
                matches!(ast.get_expr(*index),
                    Expr::String(s) if CHAIN_NAMES.iter().any(|k| s == *k))
            }
            Expr::Ident(n) => holds_a_chain(n) && !named_bases.contains(&ExprId(i as u32)),
            _ => false,
        });
}

const REACHING_KEYS: &[&str] = &[
    "defineProperty",
    "defineProperties",
    "__defineGetter__",
    "getPrototypeOf",
    "constructor",
    "assign",
];

/// The names whose value is, or reaches, a constructor whose
/// prototype sits on an array's chain.
fn holds_a_chain(name: &str) -> bool {
    CHAIN_NAMES.contains(&name)
}

const CHAIN_NAMES: &[&str] = &["Array", "Object", "Reflect", "globalThis"];

/// Functions and classes this program declares whose name no other
/// binding anywhere reuses: `F.prototype` off one of them is that
/// declaration's own object, never a builtin's.
fn own_ctors(ast: &Ast) -> HashSet<String> {
    let mut decls = HashSet::new();
    let mut other = HashSet::new();
    let mut visit = |s: &Stmt| match s {
        Stmt::FnDecl { name, params, .. } => {
            decls.insert(name.clone());
            other.extend(params.iter().map(|p| p.name.clone()));
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            decls.insert(name.clone());
            let ctor_params = ctor.iter().flat_map(|c| c.params.iter());
            let method_params = methods.iter().chain(static_methods).flat_map(|m| &m.params);
            other.extend(ctor_params.chain(method_params).map(|p| p.name.clone()));
        }
        Stmt::LetDecl { name, .. }
        | Stmt::UsingDecl { name, .. }
        | Stmt::ForOf { var_name: name, .. }
        | Stmt::ForOfSplitIter { var_name: name, .. } => {
            other.insert(name.clone());
        }
        Stmt::Try {
            catch_param: Some(name),
            ..
        } => {
            other.insert(name.clone());
        }
        _ => {}
    };
    super::walk_stmts(&ast.stmts, &mut visit);
    let mut arrow_params = Vec::new();
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            arrow_params.extend(params.iter().map(|p| p.name.clone()));
            super::walk_stmts(body, &mut visit);
        }
    }
    other.extend(arrow_params);
    decls.retain(|n| !other.contains(n));
    decls
}

/// Identifiers in a position that names a static member or calls /
/// tests the constructor, and so cannot carry the binding anywhere: a
/// member base, a call's callee, an `instanceof` right side, and a
/// keyed base whose key is a string literal (the literal itself is
/// looked at above).
fn named_bases(ast: &Ast) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                out.insert(*obj);
            }
            Expr::Call { callee, .. } => {
                out.insert(*callee);
            }
            Expr::InstanceOf { rhs, .. } => {
                out.insert(*rhs);
            }
            Expr::Index { obj, index } | Expr::OptIndex { obj, index }
                if matches!(ast.get_expr(*index), Expr::String(_)) =>
            {
                out.insert(*obj);
            }
            _ => {}
        }
    }
    out
}
