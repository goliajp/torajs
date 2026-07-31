//! Iterator-helper method reads on iterator-shaped TYPED receivers —
//! RFC 20260730-iterator-global §3.3's SSA face, checker side.
//!
//! The helpers runtime (IterHelper cell, tag 25) dispatches off the
//! any lane: `const it: any = g(); it.map(f)` already works end to
//! end. What was missing is the STATIC receiver route — `g()` types
//! as `ClassRef("__Gen_g")`, `[..].values()` as `ArrIter`, and the
//! member read hit the terminal "no member" reject before the call
//! could reach the any-lane dispatcher.
//!
//! This wedge answers `Type::Any` for the eleven §27.1.4 helper
//! names on receivers that are iterators by construction. The Any
//! record on the Member expr is exactly what the lowering keys on:
//! `ssa_lower_any_method_call`'s `any_member_read` gate boxes the
//! concrete receiver at the any-lane boundary and the runtime
//! dispatcher takes it from there (generator instances via the
//! struct-method miss-tail instanceof gate, MapIter/ArrIter via
//! their cell arms).
//!
//! Placement: AFTER `try_family_dispatch` in the member check — a
//! user override (`class C extends Iterator { map() {...} }`) or a
//! real class method wins through the earlier `__cm_` probe; this
//! is the fallback face only.

use crate::ast::Ast;
use crate::check::Type;

/// %Iterator.prototype% helper names (§27.1.4.x) — the five lazy
/// adapters plus the six eager consumers.
const ITERATOR_HELPERS: &[&str] = &[
    "map", "filter", "take", "drop", "flatMap", "toArray", "forEach", "reduce", "some", "every",
    "find",
];

/// Answer `Any` for a helper read on an iterator-shaped typed
/// receiver; `None` falls back to the terminal reject.
pub(crate) fn try_match(obj_ty: &Type, name: &str, ast: &Ast) -> Option<Result<Type, String>> {
    if !ITERATOR_HELPERS.contains(&name) {
        return None;
    }
    let iter_shaped = match obj_ty {
        Type::MapIter | Type::ArrIter => true,
        Type::ClassRef(n) => is_iterator_class(n, ast),
        _ => false,
    };
    if !iter_shaped {
        return None;
    }
    Some(Ok(Type::Any))
}

/// A class is an iterator by construction when it is a desugared
/// generator state machine (`generator_factory_classes` values) or
/// when its heritage chain reaches an `extends Iterator` record
/// (`builtin_proto_heirs` maps the stripped class to proto tag 15 =
/// %Iterator.prototype%, see `desugar_classes_builtin_heritage`).
fn is_iterator_class(n: &str, ast: &Ast) -> bool {
    if ast.generator_factory_classes.values().any(|c| c == n) {
        return true;
    }
    let mut cur = n;
    for _ in 0..64 {
        if ast.builtin_proto_heirs.get(cur) == Some(&15) {
            return true;
        }
        match ast.class_parents.get(cur) {
            Some(Some(p)) => cur = p,
            _ => return false,
        }
    }
    false
}
