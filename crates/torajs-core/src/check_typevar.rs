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
            // A fn VALUE crossing a bare-TypeVar boundary instantiates
            // at Any, never at its Function type. The checker's
            // `Type::Function` is repr-blind, but the SSA layer splits
            // fn values in two: a bare fn name is a raw `FnSig` code
            // pointer while a struct-field / fn-typed-slot load is a
            // `Closure` cell — and both reach the same generic. A
            // `Function`-instantiated clone would pick ONE ABI
            // (`__fn(...)` = raw CallIndirect) and jump through a
            // closure cell's header bytes when the other repr arrives
            // (SIGBUS — the `{ f: top_fn }` → `check(o.f)` shape).
            // Boxed-any dispatch handles both reprs at runtime; the
            // fn-name wrap axis (`mark_known_callee_args`) turns the
            // raw-FnSig arguments into boxable closure cells.
            // Fn-typed PATTERNS (`f: (t: T) => U`) don't route here —
            // they decompose structurally below and keep their reprs.
            let concrete = if matches!(concrete, Type::Function(..)) {
                &Type::Any
            } else {
                concrete
            };
            if let Some(existing) = subst.get(name) {
                // `any` absorbs in inference, in BOTH directions (TS
                // semantics): an Any binding is compatible with every
                // later actual (T stays any), and an Any actual WIDENS
                // an earlier concrete binding to any — keeping the
                // concrete binding would monomorphize the clone to raw
                // typed slot loads over an Any-repr argument, reading
                // NaN-box bits as scalars (RFC
                // 20260721-array-proto-cluster 刀 13a). The absorption
                // is STRUCTURAL: Array(Any) joins Array(Number) as
                // Array(Any) the same way Any joins Number (t262
                // dstr-rest census r283 — the rest pattern collects
                // Array(Any) while the assert's expected literal is
                // Array(Number)). Only structurally-distinct concrete
                // types conflict.
                match any_absorbing_join(existing, concrete) {
                    Some(joined) => {
                        if joined != *existing {
                            subst.insert(name.clone(), joined);
                        }
                    }
                    None => {
                        return Err(format!(
                            "type parameter `{name}` was inferred as {existing:?} earlier but here is {concrete:?}"
                        ));
                    }
                }
            } else {
                subst.insert(name.clone(), concrete.clone());
            }
            Ok(())
        }
        // `any` in pattern position admits every actual and binds
        // nothing — it is the top type here. An explicitly-`any`
        // param sits alongside TypeVars whenever only SOME params of
        // an implicit-generic fn were annotated or materialized
        // (`f(a = <expr>, y = 1)` after V2b turns `a` into
        // `a: any = undefined` while `y` stays a fresh TypeVar);
        // the structural fallback below would reject the call with
        // "expected Any, got Number".
        (Type::Any, _) => Ok(()),
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

/// Structural any-absorbing join of two inferred bindings: equal
/// types join as themselves, `Any` absorbs anything at any depth,
/// and Array joins element-wise (`Array(Any)` ⊔ `Array(Number)` =
/// `Array(Any)`). `None` = genuinely distinct concrete types — the
/// caller keeps the inference-conflict reject. Function shapes stay
/// conflict-on-difference: widening a callback signature would
/// change its calling convention, not just its repr.
fn any_absorbing_join(a: &Type, b: &Type) -> Option<Type> {
    if a == b {
        return Some(a.clone());
    }
    match (a, b) {
        (Type::Any, _) | (_, Type::Any) => Some(Type::Any),
        (Type::Array(ae), Type::Array(be)) => {
            Some(Type::Array(Box::new(any_absorbing_join(ae, be)?)))
        }
        // Cluster #6 sibling (rotation 442) — Null is a legal
        // inhabitant of every Nullable(T) and of a match result's
        // decayed Array, so those pairs join instead of conflicting
        // (the t262 harness's `sameValue(s.match(re), null)` shape,
        // both call orders). The join is Any, not the Nullable: a
        // Nullable binding monomorphizes to a bare-Arr param whose
        // retarget-site null guard would throw on the very null the
        // join just admitted, while the boxed-any lane carries null
        // end-to-end (ANY_NULL: `=== null`, print, concat all
        // measured correct). The bare-Array pair rides along because
        // an argument-position match result decays its Nullable
        // before reaching the typevar.
        (Type::Nullable(_), Type::Null)
        | (Type::Null, Type::Nullable(_))
        | (Type::Nullable(_), Type::Nullable(_))
        | (Type::Array(_), Type::Null)
        | (Type::Null, Type::Array(_)) => Some(Type::Any),
        _ => None,
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
