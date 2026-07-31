//! TypeVar inference helpers for generic call sites — pattern/actual
//! unification and occurs checks. Split out of check.rs (file-size
//! debt); pure functions over `check::Type`, no Checker state.

use crate::check::Type;
use std::collections::HashMap;

/// Walk `pattern` and `actual` in lockstep; whenever a `TypeVar(name)` is
/// found in `pattern`, bind it to the matching position in `actual`
/// (or check consistency if already bound). Returns Err on mismatch.
pub(crate) fn unify_typevar(
    pattern: &Type,
    actual: &Type,
    subst: &mut HashMap<String, Type>,
) -> Result<(), String> {
    match (pattern, actual) {
        (Type::TypeVar(name), concrete) => {
            if let Some(existing) = subst.get(name) {
                // `any` absorbs in inference, in BOTH directions (TS
                // semantics): an Any binding is compatible with every
                // later actual (T stays any), and an Any actual WIDENS
                // an earlier concrete binding to any — keeping the
                // concrete binding would monomorphize the clone to raw
                // typed slot loads over an Any-repr argument, reading
                // NaN-box bits as scalars (RFC
                // 20260721-array-proto-cluster 刀 13a). Only two
                // distinct concrete types conflict.
                if existing != concrete
                    && !matches!(existing, Type::Any)
                    && !matches!(concrete, Type::Any)
                {
                    return Err(format!(
                        "type parameter `{name}` was inferred as {existing:?} earlier but here is {concrete:?}"
                    ));
                }
                if matches!(concrete, Type::Any) && !matches!(existing, Type::Any) {
                    subst.insert(name.clone(), Type::Any);
                }
            } else {
                subst.insert(name.clone(), concrete.clone());
            }
            Ok(())
        }
        (Type::Array(p_elem), Type::Array(a_elem)) => unify_typevar(p_elem, a_elem, subst),
        (Type::Function(p_args, p_ret), Type::Function(a_args, a_ret)) => {
            // Rest-tail pattern (`(...args: T[]) => T`, RFC
            // 20260708-variadic-fn-type-ann): the fixed prefix
            // unifies positionally, then every remaining actual
            // param unifies against the rest ELEMENT — a fixed-arity
            // closure `(a: number) => ...` matches with T=number.
            if let Some(Type::Rest(elem)) = p_args.last() {
                let fixed = &p_args[..p_args.len() - 1];
                if a_args.len() < fixed.len() {
                    return Err(format!(
                        "function arity mismatch: pattern needs at least {:?} fixed argument(s), actual has {:?}",
                        fixed.len(),
                        a_args.len()
                    ));
                }
                for (pa, aa) in fixed.iter().zip(a_args.iter()) {
                    unify_typevar(pa, aa, subst)?;
                }
                for aa in &a_args[fixed.len()..] {
                    match aa {
                        Type::Rest(a_elem) => unify_typevar(elem, a_elem, subst)?,
                        other => unify_typevar(elem, other, subst)?,
                    }
                }
                return unify_typevar(p_ret, a_ret, subst);
            }
            // TS function-type compatibility — an actual that
            // accepts FEWER parameters than the pattern provides is
            // compatible (callback parameter elision: `arr.map(x =>
            // x)` against `(v, i, arr) => U`); only the common
            // prefix unifies, and a typevar living solely in an
            // elided position stays unbound (the caller's
            // could-not-infer check reports it). An actual needing
            // MORE parameters than the pattern supplies stays a
            // mismatch.
            if a_args.len() > p_args.len() {
                return Err(format!(
                    "function arity mismatch: pattern {:?}, actual {:?}",
                    p_args.len(),
                    a_args.len()
                ));
            }
            for (pa, aa) in p_args.iter().zip(a_args.iter()) {
                unify_typevar(pa, aa, subst)?;
            }
            unify_typevar(p_ret, a_ret, subst)
        }
        (Type::Struct(p_fields), Type::Struct(a_fields)) => {
            if p_fields.len() != a_fields.len() {
                return Err(format!(
                    "struct field count mismatch: pattern {} fields, actual {}",
                    p_fields.len(),
                    a_fields.len()
                ));
            }
            for ((pn, pt), (an, at)) in p_fields.iter().zip(a_fields.iter()) {
                if pn != an {
                    return Err(format!(
                        "struct field name mismatch: expected `{pn}`, got `{an}`"
                    ));
                }
                unify_typevar(pt, at, subst)?;
            }
            Ok(())
        }
        (Type::Nullable(p), Type::Nullable(a)) => unify_typevar(p, a, subst),
        (a, b) if a == b => Ok(()),
        (a, b) => Err(format!("expected {a:?}, got {b:?}")),
    }
}

/// Replace every `TypeVar(name)` inside `ty` with the binding from `subst`.
/// Used to compute the resolved return type at a generic call site.
/// T-28 — does TypeVar `name` appear anywhere inside `ty`? Used by
/// the implicit-generic-fn arity-pad path to verify that trailing
/// missing TypeVars don't bind anything else (so binding them to Any
/// is safe).
pub(crate) fn typevar_appears_in(ty: &Type, name: &str) -> bool {
    match ty {
        Type::TypeVar(n) => n == name,
        Type::Array(inner) => typevar_appears_in(inner, name),
        Type::Function(args, ret) => {
            args.iter().any(|t| typevar_appears_in(t, name)) || typevar_appears_in(ret, name)
        }
        Type::Struct(fields) => fields.iter().any(|(_, t)| typevar_appears_in(t, name)),
        Type::Nullable(inner) => typevar_appears_in(inner, name),
        _ => false,
    }
}

pub(crate) fn typevar_appears_in_iter(tys: &[Type], name: &str) -> bool {
    tys.iter().any(|t| typevar_appears_in(t, name))
}
