//! `substitute_typevars` pulled out of [`crate::check`] as chunk-322
//! of the check.rs god-file decomp.
//!
//! M3 — pure recursive walker that substitutes `Type::TypeVar(name)`
//! occurrences with the corresponding entry in a `subst` map. Used by
//! the generic-fn call-site monomorphizer
//! (`check_type_of_call_generic_ident`) to resolve the return type
//! against the inferred per-call type arguments.
//!
//! Recurses through wrapper variants `Array` / `Function` (args + ret)
//! / `Struct` (named fields); other types pass through cloned.
//! Re-exported back into `crate::check` so the canonical
//! `crate::check::substitute_typevars` path keeps working.

use crate::check::Type;
use std::collections::HashMap;

pub(crate) fn substitute_typevars(ty: &Type, subst: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeVar(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(inner) => Type::Array(Box::new(substitute_typevars(inner, subst))),
        Type::Function(args, ret) => Type::Function(
            args.iter().map(|t| substitute_typevars(t, subst)).collect(),
            Box::new(substitute_typevars(ret, subst)),
        ),
        Type::Struct(fields) => Type::Struct(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), substitute_typevars(t, subst)))
                .collect(),
        ),
        Type::Rest(inner) => Type::Rest(Box::new(substitute_typevars(inner, subst))),
        // Blade 3 (RFC 20260815-generic-nominal-identity) — a nominal
        // generic-class reference carries its type params INSIDE the
        // key string (`ClassRef("Box<T>")` from the factory's `Box<T>`
        // return ann). Word-level substitution rewrites the key to the
        // instantiated spelling (`"Box<number>"`) — the same one the
        // lowering's `substitute_in_ann` mints, so the ann round-trip
        // lands on the same `inst_memo` layout.
        Type::ClassRef(name) if name.contains('<') => {
            let pairs: Vec<(String, String)> = subst
                .iter()
                .map(|(k, v)| (k.clone(), crate::check_type_to_ann::type_to_ann(v)))
                .collect();
            Type::ClassRef(crate::ssa_lower_generics_monomorph::substitute_in_ann(
                name, &pairs,
            ))
        }
        other => other.clone(),
    }
}
