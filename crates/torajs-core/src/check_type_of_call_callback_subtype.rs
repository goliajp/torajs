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
    matches(&resolve_sig(param_ty), &resolve_sig(arg_ty))
}

pub(crate) fn matches(param_ty: &Type, arg_ty: &Type) -> bool {
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
                || matches!(actual_ret.as_ref(), Type::Any))
                && actual_ps.iter().enumerate().all(|(i, a)| {
                    let f = if i < fixed.len() {
                        &fixed[i]
                    } else {
                        elem.as_ref()
                    };
                    a == f || matches!(f, Type::Any) || matches!(a, Type::Any)
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
                || actual_ps[formal_ps.len()..]
                    .iter()
                    .all(|a| matches!(a, Type::Any));
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
                    || matches!(formal_ret.as_ref(), Type::Any | Type::Void | Type::Undefined)
                    || matches!(actual_ret.as_ref(), Type::Any))
                && actual_ps
                    .iter()
                    .zip(formal_ps.iter())
                    .all(|(a, f)| a == f || matches!(f, Type::Any) || matches!(a, Type::Any))
        }
        _ => false,
    }
}
