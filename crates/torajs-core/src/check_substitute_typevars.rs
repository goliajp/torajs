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
        other => other.clone(),
    }
}
