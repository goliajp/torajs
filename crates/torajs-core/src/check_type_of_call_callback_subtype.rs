//! S133 callback Function subtype check extracted from
//! [`crate::check_type_of_call::check`]'s per-arg type-
//! check loop body (chunk 307 — ninety-ninth sub-batch
//! of check_type_of_call.rs per-shape decomposition).
//!
//! JS spec lets a callback accept fewer args than the
//! formal `Function` param declares (e.g. `Map.forEach`:
//! `(v) =>` is legal even though spec sig is
//! `(v, k, map) => void`). Strict equality on
//! `Type::Function` would reject any shorter callback.
//! Accept when:
//!
//! 1. Actual arity ≤ formal arity (callback may ignore
//!    trailing args)
//! 2. Return type matches, or either side is `Any` (user
//!    callback without type-ann defaults to `Any`, accepts
//!    against typed formal; formal `Any` accepts any typed
//!    actual)
//! 3. Every prefix slot matches with either side being
//!    `Any` (same widening as the return slot)
//!
//! Non-Function param / non-Function arg → false (the
//! caller falls through to strict-equality check).
//!
//! Pure: no mutation, no side effects. Returns `true` iff
//! the subtype carve-out applies.

use crate::check::{GenericAliasMap, Type};

/// [`matches`], but resolving one level of `ClassRef` inside the two
/// signatures first.
///
/// [`crate::check_type_of_call::general::arg_admitted`] resolves the
/// argument and parameter types before comparing them — "two
/// DIFFERENT class names still compare structurally, which is what TS
/// assignability is". That resolve is **top-level only**, and a
/// `ClassRef` sitting in a callback's parameter list is not top-level,
/// so it never reached it:
///
/// ```ts
/// class Box { v: number; constructor(v: number) { this.v = v; } }
/// const bs = [new Box(1), new Box(2)];
/// bs.filter(b => b.v > 1);
/// ```
///
/// The array's element type arrives already resolved
/// (`Struct([("v", Number)])`) while the arrow's inferred parameter
/// stays `ClassRef("Box")`, so `a == f` compared a name against a
/// shape and answered false — a compile error on a program every
/// engine runs. It reached `filter` / `find` / `findLast` /
/// `findIndex` / `findLastIndex` / `some` / `every` / `forEach`.
/// `map` escaped for a reason that had nothing to do with being
/// right: `crate::check_type_of_call_arr_map_hetero` intercepts a
/// `.map` whose callback returns something other than the element
/// type, and answers before the general loop ever compares the two
/// signatures.
///
/// Resolution stays **one level deep**, matching
/// `resolve_class_ref`'s own contract: it deliberately leaves
/// `ClassRef` nodes embedded in struct and array fields alone,
/// because a fully-resolved self-referential class would expand
/// forever. This walks the two parameter lists and the two return
/// slots, nothing further down.
pub(crate) fn matches_resolved(
    param_ty: &Type,
    arg_ty: &Type,
    class_structs: &std::collections::HashMap<String, Type>,
    aliases: &std::collections::HashMap<String, Type>,
    generic_aliases: &GenericAliasMap,
) -> bool {
    let resolve_sig = |t: &Type| -> Type {
        let Type::Function(ps, ret) = t else {
            return t.clone();
        };
        let one =
            |x: &Type| crate::check::resolve_class_ref(x, class_structs, aliases, generic_aliases);
        Type::Function(
            ps.iter()
                .map(|p| match p {
                    // Keep the Rest sentinel intact — the variadic arm
                    // pattern-matches on it — and resolve its element.
                    Type::Rest(elem) => Type::Rest(Box::new(one(elem))),
                    other => one(other),
                })
                .collect(),
            Box::new(one(ret)),
        )
    };
    matches_face(&resolve_sig(param_ty), &resolve_sig(arg_ty), true)
}

/// Assignment-position face (let / assign / field / array-literal
/// unification through `check_assignable`): the S133 shorter-arity
/// admit WITHOUT the S2 call-face relaxations. The two S2 arms
/// (excess-Any-tail arity, void-formal ret) are deliberately
/// call-arg-only — as a unification proxy the wider admit collapsed
/// previously-heterogeneous array literals (`[{then(a)}, {then(a,
/// b)}]` went Array<Struct> instead of Array<Any>) and downstream
/// arms keyed on that shape (the Promise.all thenable family,
/// rotation 355 sweep regression).
pub(crate) fn matches(param_ty: &Type, arg_ty: &Type) -> bool {
    matches_face(param_ty, arg_ty, false)
}

fn matches_face(param_ty: &Type, arg_ty: &Type, call_face: bool) -> bool {
    match (param_ty, arg_ty) {
        // RFC 20260708-variadic — formal ends with the Rest(elem)
        // sentinel (`(...args: E[]) => R`): the actual callback may
        // declare any arity; prefix slots pair against the fixed
        // formals, overflow slots against the element type (Any
        // widening on both, same as the fixed-arity arm below).
        (Type::Function(formal_ps, formal_ret), Type::Function(actual_ps, actual_ret))
            if matches!(formal_ps.last(), Some(Type::Rest(_))) =>
        {
            let Some(Type::Rest(elem)) = formal_ps.last() else {
                unreachable!("guarded by the matches! above");
            };
            let fixed = &formal_ps[..formal_ps.len() - 1];
            (formal_ret.as_ref() == actual_ret.as_ref()
                || matches!(formal_ret.as_ref(), Type::Any)
                || matches!(actual_ret.as_ref(), Type::Any | Type::Never))
                && actual_ps.iter().enumerate().all(|(i, a)| {
                    let f = if i < fixed.len() {
                        &fixed[i]
                    } else {
                        elem.as_ref()
                    };
                    slot_agrees(a, f)
                })
        }
        (Type::Function(formal_ps, formal_ret), Type::Function(actual_ps, actual_ret)) => {
            // RFC 20260810-indirect-argc-abi S2 — the REVERSE arity
            // direction: a callback may also declare MORE params than
            // the formal face, when every excess slot is Any. The
            // §10.2.1.4 argument binding gives unpassed positions
            // `undefined`, which the S2 callee-side argc
            // normalization delivers into exactly those Any slots
            // (a typed excess slot has no undefined repr — stays
            // rejected). This is the `assert.throws(SyntaxError, f)`
            // face: f declares a defaulted param, the harness's
            // Function-typed slot declares none.
            let arity_ok = actual_ps.len() <= formal_ps.len()
                || (call_face
                    && actual_ps[formal_ps.len()..]
                        .iter()
                        .all(|a| matches!(a, Type::Any | Type::Nullable(_))));
            // S2 also adds the TS void-return exception: a
            // `() => void` face accepts a value-returning callback —
            // the call site lowers through the formal sig and
            // discards the result, exactly what the Any-actual-ret
            // arm below has always done (`assert.throws(SyntaxError,
            // genFn)`: the generator factory's return is ignored).
            // Undefined is Void's checker alias (general.rs retypes
            // Void calls Undefined), so both spellings admit.
            arity_ok
                && (formal_ret.as_ref() == actual_ret.as_ref()
                    || matches!(formal_ret.as_ref(), Type::Any)
                    || (call_face && matches!(formal_ret.as_ref(), Type::Void | Type::Undefined))
                    || matches!(actual_ret.as_ref(), Type::Any | Type::Never))
                && actual_ps
                    .iter()
                    .zip(formal_ps.iter())
                    .all(|(a, f)| slot_agrees(a, f))
        }
        _ => false,
    }
}

/// Whether a callback ending in a rest parameter (`(a, ...xs: E[])`)
/// takes every argument of a call passing `formals`: its fixed
/// parameters pair positionally, the rest element against each formal
/// past them — TS's rule for a rest source.
pub(crate) fn rest_callback_fits(formals: &[Type], actual: &[Type]) -> bool {
    let Some((Type::Rest(elem), fixed)) = actual.split_last() else {
        return false;
    };
    fixed.len() <= formals.len()
        && formals
            .iter()
            .enumerate()
            .all(|(i, f)| slot_agrees(fixed.get(i).unwrap_or(elem), f))
}

/// A callback parameter slot matches its formal: equal, either side
/// Any, or an object formal assignable to the object it declares — and a `Map` matches component-wise by the same rule, so
/// `(v, k, m: Map<string, number>) => …` fits a forEach face built
/// from an unannotated `Map<any, any>` receiver.
fn slot_agrees(a: &Type, f: &Type) -> bool {
    match (a, f) {
        _ if a == f => true,
        (Type::Any, _) | (_, Type::Any) => true,
        // an optional callback param (`i?: number`) takes every value
        // the formal passes
        (Type::Nullable(inner), _) => slot_agrees(inner, f),
        // a union parameter takes the formal's value when one member does
        (Type::Union(members), _) => members.iter().any(|m| slot_agrees(m, f)),
        (Type::Map(ak, av), Type::Map(fk, fv)) => slot_agrees(ak, fk) && slot_agrees(av, fv),
        (Type::MapIter(ai), Type::MapIter(fi)) => slot_agrees(ai, fi),
        // parameters are contravariant: a callback declaring a wider
        // object parameter takes every narrower object the formal
        // passes; the prefix layout makes the cell readable as either
        (Type::Struct(_), Type::Struct(_)) => crate::check_assignable::is_assignable_to(a, f),
        _ => false,
    }
}
