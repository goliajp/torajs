//! TypeVar inference helpers for generic call sites — pattern/actual
//! unification and occurs checks. Split out of check.rs (file-size
//! debt); pure functions over `check::Type` plus the class parent map,
//! which is what tells two class types whether one is the other's
//! ancestor. No Checker state.

use crate::check::Type;
use std::collections::HashMap;

/// Walk `pattern` and `actual` in lockstep; whenever a `TypeVar(name)` is
/// found in `pattern`, bind it to the matching position in `actual`
/// (or check consistency if already bound). Returns Err on mismatch.
pub(crate) fn unify_typevar(
    pattern: &Type,
    actual: &Type,
    subst: &mut HashMap<String, Type>,
    parents: &HashMap<String, Option<String>>,
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
                match any_absorbing_join(existing, concrete, parents) {
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
        (Type::Array(p_elem), Type::Array(a_elem)) => unify_typevar(p_elem, a_elem, subst, parents),
        // An unannotated `new Map()` is `Map<any, any>`: an Any
        // component fits a concrete one (the assignability arm's
        // rule) and still binds a type variable the usual way.
        (Type::Map(pk, pv), Type::Map(ak, av)) => {
            for (p, a) in [(pk, ak), (pv, av)] {
                if !(matches!(**a, Type::Any) && !matches!(**p, Type::TypeVar(_))) {
                    unify_typevar(p, a, subst, parents)?;
                }
            }
            Ok(())
        }
        (Type::MapIter(p), Type::MapIter(a)) if matches!(**a, Type::Any) => {
            if matches!(**p, Type::TypeVar(_)) {
                unify_typevar(p, a, subst, parents)
            } else {
                Ok(())
            }
        }
        (Type::MapIter(p), Type::MapIter(a)) => unify_typevar(p, a, subst, parents),
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
                    unify_typevar(pa, aa, subst, parents)?;
                }
                for aa in &a_args[fixed.len()..] {
                    match aa {
                        Type::Rest(a_elem) => unify_typevar(elem, a_elem, subst, parents)?,
                        other => unify_typevar(elem, other, subst, parents)?,
                    }
                }
                return unify_typevar(p_ret, a_ret, subst, parents);
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
                unify_typevar(pa, aa, subst, parents)?;
            }
            unify_typevar(p_ret, a_ret, subst, parents)
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
                        "struct field name mismatch: expected `{}`, got `{}`",
                        pn.lossy(),
                        an.lossy()
                    ));
                }
                unify_typevar(pt, at, subst, parents)?;
            }
            Ok(())
        }
        (Type::Nullable(p), Type::Nullable(a)) => unify_typevar(p, a, subst, parents),
        // a dictionary pattern reads its key and value types off a
        // dictionary, an object type (string-named properties, those
        // the key covers) or an array (index keys)
        (Type::Dict(pk, pv), actual) => {
            let (key, values): (Type, Vec<&Type>) = match actual {
                Type::Dict(ak, av) => ((**ak).clone(), vec![&**av]),
                Type::Struct(fields) => (
                    Type::String,
                    fields
                        .iter()
                        .filter(|(n, _)| pk.dict_key_covers(&n.lossy()))
                        .map(|(_, t)| t)
                        .collect(),
                ),
                Type::Array(e) => (Type::Number, vec![&**e]),
                Type::Any => return Ok(()),
                _ => return Err(format!("expected {pattern:?}, got {actual:?}")),
            };
            if matches!(**pk, Type::TypeVar(_)) {
                unify_typevar(pk, &key, subst, parents)?;
            }
            values
                .into_iter()
                .try_for_each(|v| unify_typevar(pv, v, subst, parents))
        }
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
fn any_absorbing_join(
    a: &Type,
    b: &Type,
    parents: &HashMap<String, Option<String>>,
) -> Option<Type> {
    if a == b {
        return Some(a.clone());
    }
    match (a, b) {
        (Type::Any, _) | (_, Type::Any) => Some(Type::Any),
        (Type::Array(ae), Type::Array(be)) => {
            Some(Type::Array(Box::new(any_absorbing_join(ae, be, parents)?)))
        }
        (Type::Map(ak, av), Type::Map(bk, bv)) => Some(Type::Map(
            Box::new(any_absorbing_join(ak, bk, parents)?),
            Box::new(any_absorbing_join(av, bv, parents)?),
        )),
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
        // 509-03 — a subclass and its ancestor are not two answers to
        // `T`, they are one: every value of the subclass is a value of
        // the ancestor, so the ancestor is what `T` was all along. The
        // t262 harness's `sameValue<T>(a: T, b: T)` hands over
        // `Y.prototype.method()`'s `X` and then a `Y`, which TS binds
        // as `X` and tr refused as a conflict the moment rotation 509
        // gave the dispatch stub its rows' return type. Unrelated
        // classes still conflict — their join is not a class.
        // The Undefined sibling of the Null rows above, on the same
        // reasoning and wider. `undefined` stands beside every
        // concrete type as its absence: TS binds `T` for
        // `sameValue(x[2], undefined)` — a read past an array's
        // length — as `number | undefined`, and Any is tr's one repr
        // that carries both. Null keeps its narrow measured rows;
        // this row is the whole type domain, because a beyond-length
        // read and an absent argument put `undefined` next to every
        // concrete type the suite has.
        (Type::Undefined, _) | (_, Type::Undefined) => Some(Type::Any),
        (Type::ClassRef(x), Type::ClassRef(y)) => class_join(x, y, parents),
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
        Type::Array(inner) | Type::MapIter(inner) => typevar_appears_in(inner, name),
        Type::Map(k, v) | Type::Dict(k, v) => {
            typevar_appears_in(k, name) || typevar_appears_in(v, name)
        }
        Type::Function(args, ret) => {
            args.iter().any(|t| typevar_appears_in(t, name)) || typevar_appears_in(ret, name)
        }
        Type::Struct(fields) => fields.iter().any(|(_, t)| typevar_appears_in(t, name)),
        Type::Nullable(inner) => typevar_appears_in(inner, name),
        _ => false,
    }
}

/// Bind every type variable inside `ty` that `subst` has no binding
/// for yet to `any`.
pub(crate) fn bind_unbound_to_any(ty: &Type, subst: &mut HashMap<String, Type>) {
    match ty {
        Type::TypeVar(n) => {
            subst.entry(n.clone()).or_insert(Type::Any);
        }
        Type::Array(inner) | Type::MapIter(inner) | Type::Nullable(inner) | Type::Rest(inner) => {
            bind_unbound_to_any(inner, subst)
        }
        Type::Map(k, v) | Type::Dict(k, v) => {
            bind_unbound_to_any(k, subst);
            bind_unbound_to_any(v, subst);
        }
        Type::Function(args, ret) => {
            args.iter().for_each(|t| bind_unbound_to_any(t, subst));
            bind_unbound_to_any(ret, subst);
        }
        Type::Struct(fields) => fields
            .iter()
            .for_each(|(_, t)| bind_unbound_to_any(t, subst)),
        _ => {}
    }
}

pub(crate) fn typevar_appears_in_iter(tys: &[Type], name: &str) -> bool {
    tys.iter().any(|t| typevar_appears_in(t, name))
}

/// The nearer of two class names when one descends from the other,
/// and nothing when they are unrelated. A `ClassRef` key carries its
/// type arguments in a `<...>` tail; the chain is spelled without
/// them.
fn class_join(x: &str, y: &str, parents: &HashMap<String, Option<String>>) -> Option<Type> {
    let bare = |k: &str| k.split('<').next().unwrap_or(k).to_string();
    let (bx, by) = (bare(x), bare(y));
    if crate::ast::method_owner_is_in_chain(parents, &bx, &by) {
        Some(Type::ClassRef(x.to_string()))
    } else if crate::ast::method_owner_is_in_chain(parents, &by, &bx) {
        Some(Type::ClassRef(y.to_string()))
    } else {
        None
    }
}
