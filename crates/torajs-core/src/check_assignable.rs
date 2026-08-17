//! Assignability lattice — `is_assignable_to_resolved` and its
//! recursive core, split out of check.rs (file-size known-debt:
//! check.rs only shrinks). Every site that compares a declared slot
//! type against an inferred value type (LetDecl init, Assign LHS/RHS,
//! fn-arg coercion, return-value compat) goes through this module.

use crate::check::{GenericAliasMap, Type, resolve_class_ref};

/// Subtyping rule for the `let x: T = init` shape and similar slots.
/// Returns true iff a value of type `from` is assignable to a variable
/// of type `to`. The only widening relations we admit so far:
///
/// - `T == T`                                — identity
/// - `T → T | null` (Nullable widening)      — non-null T fits a nullable slot
/// - `null → T | null`                       — null fits a nullable slot
/// - `null → null`                           — identity (rare in practice)
///
/// Everything else falls back to PartialEq. Notably we do NOT auto-narrow
/// `T | null → T` — the user must use `??` or `?.` to dispose of the null.
/// V3-05 — caller-side resolver wrapper. Use this at every site
/// where the operands may be class types whose ClassRef placeholder
/// hasn't been dereferenced yet (LetDecl init, Assign LHS/RHS, fn-
/// arg coercion, return-value compat). Resolving up-front keeps the
/// existing `is_assignable_to` body free of alias-table threading.
///
/// Deep-resolves through Struct fields too — the V3-06 case
/// `class C { kids: C[] }` needs `Array(ClassRef("C"))` to match
/// `Array(Struct(...))` recursively. A `seen` cycle guard keyed by
/// `(to_class, from_class)` keeps recursive class layouts finite.
pub fn is_assignable_to_resolved(
    to: &Type,
    from: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> bool {
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    is_assignable_to_deep(to, from, class_structs, aliases, generic_aliases, &mut seen)
}

fn is_assignable_to_deep(
    to: &Type,
    from: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
    seen: &mut std::collections::HashSet<(String, String)>,
) -> bool {
    // Resolve any ClassRef placeholders one layer up before deeper
    // structural comparison.
    let to_r = resolve_class_ref(to, class_structs, aliases, generic_aliases);
    let from_r = resolve_class_ref(from, class_structs, aliases, generic_aliases);
    if to_r == from_r {
        return true;
    }
    if matches!(from_r, Type::Any) {
        return true;
    }
    // P0 — anything is assignable to Any (per TS spec). The init's
    // concrete value gets boxed into the universal Any-box at lower
    // time. Pre-fix tora rejected `let x: any = 5` with a strict
    // Any vs Number mismatch, blocking the implicit-any path the
    // entire untyped-JS surface needs.
    if matches!(to_r, Type::Any) {
        return true;
    }
    // Chunk 806 — `undefined` fits a void slot (TS: a void-returning
    // fn may `return voidCall()` / `return undefined`). Load-bearing
    // case: a forwarder body `return target(...)` around a void
    // target — the call now types Undefined (general_call) while the
    // forwarder's declared ret stays Void.
    if matches!(to_r, Type::Void) && matches!(from_r, Type::Undefined) {
        return true;
    }
    if let Type::Nullable(inner) = &to_r {
        // P1.7 — `Nullable<T>` ≡ `T | null | undefined` per spec.
        // Both null and undefined are valid in any Nullable<T> slot.
        if matches!(from_r, Type::Null | Type::Undefined) {
            return true;
        }
        return is_assignable_to_deep(
            inner,
            &from_r,
            class_structs,
            aliases,
            generic_aliases,
            seen,
        );
    }
    if let (Type::Array(to_el), Type::Array(from_el)) = (&to_r, &from_r) {
        if matches!(**to_el, Type::Any) {
            return true;
        }
        return is_assignable_to_deep(
            to_el,
            from_el,
            class_structs,
            aliases,
            generic_aliases,
            seen,
        );
    }
    // Fn-typed pairings (rest-tail AND fixed-arity) delegate to the
    // callback-subtype lattice in the shallow layer's tail arm — the
    // deep layer only needs to fall through with resolved types.
    // `Promise<T>` is covariant in T, same shape as the Array arm.
    // The load-bearing case is `Promise(Any) ← Promise(Number)`: a
    // default-Any async fn's tail-safety return constructs
    // `Promise.resolve(0)` (typed Promise(Number)), which must fit
    // the declared Promise(Any). Pre-fix both layers fell through
    // to strict equality and false-rejected legal async fns whose
    // bodies await concrete-typed promises.
    if let (Type::Promise(to_inner), Type::Promise(from_inner)) = (&to_r, &from_r) {
        return is_assignable_to_deep(
            to_inner,
            from_inner,
            class_structs,
            aliases,
            generic_aliases,
            seen,
        );
    }
    if let (Type::Struct(to_fields), Type::Struct(from_fields)) = (&to_r, &from_r)
        && from_fields.len() >= to_fields.len()
    {
        // V3-06 cycle guard: structurally-recursive layouts (`Tree`
        // contains `Array<Tree>`) would otherwise infinite-loop here.
        // We approximate identity by field-name signature — good
        // enough since the code path only fires inside a single
        // alias-resolve chain.
        let fingerprint = |fs: &[(String, Type)]| -> String {
            fs.iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        let key = (fingerprint(to_fields), fingerprint(from_fields));
        if !seen.insert(key.clone()) {
            return true;
        }
        let result = to_fields.iter().enumerate().all(|(i, (n, t))| {
            let (fn_name, fn_ty) = &from_fields[i];
            fn_name == n
                && is_assignable_to_deep(t, fn_ty, class_structs, aliases, generic_aliases, seen)
        });
        seen.remove(&key);
        return result;
    }
    is_assignable_to(&to_r, &from_r)
}

pub(crate) fn is_assignable_to(to: &Type, from: &Type) -> bool {
    if to == from {
        return true;
    }
    // M6.3 — `Type::Any` from JSON.parse return is a typecheck-level
    // hole; ssa_lower's LetDecl arm specializes the actual decode at
    // lower time using the slot's annotation. Allow Any → any T at
    // assignment sites so `let v: T = JSON.parse(text)` typechecks.
    // (`Type::Any` was previously only used as `console.log`'s param
    // type, where source Any was never the from-side; this widens
    // it without breaking that path.)
    if matches!(from, Type::Any) {
        return true;
    }
    // T-11 (v0.4.0) — `Array<Any>` is the universal element-type
    // sink; any concrete `Array<T>` widens into it via boxing at
    // ssa_lower time. Used by the synthesized `let
    // __torajs_arguments: any[] = [...params]` and by user-written
    // `let xs: any[] = [...]` whose elements happen to share a
    // concrete type.
    if let (Type::Array(to_el), Type::Array(_)) = (to, from)
        && matches!(**to_el, Type::Any)
    {
        return true;
    }
    if let Type::Nullable(inner) = to {
        // P1.7 — `Nullable<T>` ≡ `T | null | undefined` per spec.
        if matches!(from, Type::Null | Type::Undefined) {
            return true;
        }
        return is_assignable_to(inner, from);
    }
    // Phase H.2 — struct prefix subtyping. `class Sub extends Base`
    // desugars to a Sub struct whose field list starts with Base's
    // (parent fields prepended in desugar_classes), so Sub is
    // assignable to Base iff Base's fields are a prefix of Sub's
    // and pairwise types match. Pure structural rule — no class_parents
    // lookup needed; the layout invariant guarantees the fields-prefix
    // check coincides with the class hierarchy.
    if let (Type::Struct(to_fields), Type::Struct(from_fields)) = (to, from)
        && from_fields.len() >= to_fields.len()
    {
        // V3-05 — equal-length structs participate in field-by-field
        // assignability too, not just prefix-subtyping. This is what
        // lets `{v: number, next: null}` (object literal) assign into
        // `{v: number, next: Node | null}` (declared class type), and
        // what makes `b: Node` assign into a Nullable(Node) field.
        for (i, (n, t)) in to_fields.iter().enumerate() {
            let (fn_name, fn_ty) = &from_fields[i];
            if fn_name != n || !is_assignable_to(t, fn_ty) {
                return false;
            }
        }
        return true;
    }
    // Array<Sub> → Array<Base> covariance: required for heterogeneous
    // arrays like `Animal[] = [new Animal(), new Dog()]`. Same
    // structural reasoning as the struct case — both Sub and Base
    // share the storage shape (8-byte ptr slots) so the runtime
    // layout is uniform.
    if let (Type::Array(to_elem), Type::Array(from_elem)) = (to, from) {
        return is_assignable_to(to_elem, from_elem);
    }
    // `Promise<T>` covariance, mirroring the deep layer's arm for
    // callers that enter the shallow lattice directly (Nullable /
    // Struct field recursion lands here with already-resolved types).
    if let (Type::Promise(to_inner), Type::Promise(from_inner)) = (to, from) {
        return is_assignable_to(to_inner, from_inner);
    }
    // L3b #4 — fn values ride the callback-subtype lattice at EVERY
    // assignment position, not just call args: TS lets a function
    // ignore trailing parameters (`(x) => x` fits a `(a, b) =>
    // number` slot — Map.forEach's `(v) =>` is the canonical shape).
    // The call layer has admitted this since S133; let / assign /
    // field positions fell through to strict equality and rejected
    // the same pairing. One predicate, all positions.
    if matches!(to, Type::Function(..)) && matches!(from, Type::Function(..)) {
        return crate::check_type_of_call_callback_subtype::matches(to, from);
    }
    false
}

/// 423-03 ④ — the fn-face admit for a DECLARED fn-typed slot (let /
/// assign positions): the S2 call-face relaxations (excess-Any-tail
/// arity, void-formal return) apply, because the sig-thunk pass
/// reabstracts the admitted mismatched value exactly like a call
/// argument. Deliberately a SEPARATE predicate from the
/// `is_assignable_to` fn arm above — that one also serves
/// array-literal unification, where the wider admit collapsed
/// previously-heterogeneous literals (the rotation-355 regression).
pub(crate) fn fn_slot_admits(
    to: &Type,
    from: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &crate::check::GenericAliasMap,
) -> bool {
    matches!(to, Type::Function(..))
        && crate::check_type_of_call_callback_subtype::matches_resolved(
            to,
            from,
            class_structs,
            aliases,
            generic_aliases,
        )
}
