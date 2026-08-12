//! Cascade segment 6/6 of [`crate::check_type_of_call::check`]'s
//! early-route chain. Wedge order preserved verbatim from the
//! pre-split cascade; segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings — relative order of
//! every arm is unchanged.
//!
//! Covers: object_static_meta / mapset_query / weak_collection / set_ops /
//! object_getownpropdesc / bigint_asint / object_fromentries /
//! string_search_short_circuit / weakref_deref / string_search_trailing /
//! string_repeat_undef / string_replace_undef / array_with_any /
//! array_with_trailing / string_replace_split_trailing

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

// CARVE-OUT: dispatch table — 15 mechanically-ordered `try_match` wedges,
// each one `if let Some(r) = …::try_match(…) { return r }` and nothing
// else. Same shape and same reason as the sibling `route_arity_widen`:
// wedge order is semantically load-bearing (an earlier wedge pre-empts a
// later, more general shape), and this file's own module doc records that
// the segment boundaries are mechanical, NOT semantic regroupings — so
// splitting the cascade would either perturb acceptance or invent a
// grouping the pre-split order never had. Grows one line per new wedge.

pub(crate) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // S265 — `Object.{getPrototypeOf,isExtensible,isSealed,
    // preventExtensions,seal}(obj, ...trailing)` Object-
    // namespace meta-method trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_object_static_meta`]
    // (chunk 271).
    if let Some(r) =
        crate::check_type_of_call_object_static_meta::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S264 — `(Set|Map).{has,delete}(key, ...trailing)` /
    // `Map.get(key, ...trailing)` / `(Set|Map).clear(...trailing)`
    // Set/Map-receiver instance-method trailing-arg ignore
    // wedge extracted to
    // [`crate::check_type_of_call_mapset_query`]
    // (chunk 272).
    if let Some(r) = crate::check_type_of_call_mapset_query::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S301 — `WeakMap.{set,get,has,delete}` /
    // `WeakSet.{add,has,delete}` WeakMap/WeakSet-receiver
    // instance-method trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_weak_collection`]
    // (chunk 273).
    if let Some(r) =
        crate::check_type_of_call_weak_collection::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S318 — `Set.{isSubsetOf,isSupersetOf,isDisjointFrom,
    // union,intersection,difference,symmetricDifference}
    // (other, ...trailing)` Set ES2025 setops trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_set_ops`]
    // (chunk 274).
    if let Some(r) = crate::check_type_of_call_set_ops::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S315 — `Object.getOwnPropertyDescriptor(obj, key,
    // ...trailing)` Object-namespace 2-arg-spec trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_object_getownpropdesc`]
    // (chunk 275).
    if let Some(r) =
        crate::check_type_of_call_object_getownpropdesc::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S314 — `BigInt.{asIntN,asUintN}(bits, value,
    // ...trailing)` BigInt-namespace 2-arg-spec trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_bigint_asint`]
    // (chunk 276).
    if let Some(r) = crate::check_type_of_call_bigint_asint::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S309 — `Object.fromEntries(entries, ...trailing)`
    // Object-namespace 1-arg-spec trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_object_fromentries`]
    // (chunk 277).
    if let Some(r) =
        crate::check_type_of_call_object_fromentries::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // Chunk 800 — `String.search(RegExp)` per ES §22.1.3.19
    // ([`crate::check_type_of_call_string_search_regex`]); the
    // method table's (String) -> Number entry covers only the
    // plain-string needle.
    if let Some(r) =
        crate::check_type_of_call_string_search_regex::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S210 — `String.search()` / `String.search(undefined)`
    // short-circuit wedge extracted to
    // [`crate::check_type_of_call_string_search_short_circuit`]
    // (chunk 278).
    if let Some(r) =
        crate::check_type_of_call_string_search_short_circuit::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S325 — `WeakRef.deref(...trailing)` trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_weakref_deref`]
    // (chunk 279).
    if let Some(r) = crate::check_type_of_call_weakref_deref::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S324 — `String.search(needle, ...trailing)` trailing-
    // arg ignore wedge extracted to
    // [`crate::check_type_of_call_string_search_trailing`]
    // (chunk 280).
    if let Some(r) =
        crate::check_type_of_call_string_search_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S209 — `String.repeat(undefined)` 1-arg-undef-count
    // widen wedge extracted to
    // [`crate::check_type_of_call_string_repeat_undef`]
    // (chunk 281).
    if let Some(r) =
        crate::check_type_of_call_string_repeat_undef::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S207 — `String.replace` / `String.replaceAll` with
    // fewer-than-2-arg widen wedge extracted to
    // [`crate::check_type_of_call_string_replace_undef`]
    // (chunk 282).
    if let Some(r) =
        crate::check_type_of_call_string_replace_undef::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S339 — `xs.with(Any idx, val)` Any-index widen wedge
    // extracted to [`crate::check_type_of_call_array_with_any`]
    // (chunk 283).
    if let Some(r) = crate::check_type_of_call_array_with_any::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S283 — Array.prototype.with(index, value, ...trailing)
    // 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_array_with_trailing`]
    // (chunk 284).
    if let Some(r) =
        crate::check_type_of_call_array_with_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S282 — String.{replace,replaceAll,split}(useful, useful,
    // ...trailing) 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_string_replace_split_trailing`]
    // (chunk 285).
    if let Some(r) = crate::check_type_of_call_string_replace_split_trailing::try_match(
        checker, ast, callee, args,
    ) {
        return Some(r);
    }
    // RC-1 (RFC 20260706-test262-bug-corpus) — Array predicate
    // methods × Void-ret callback ToBoolean acceptance wedge —
    // see [`crate::check_type_of_call_arr_pred_void_cb`].
    if let Some(r) =
        crate::check_type_of_call_arr_pred_void_cb::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // Rotation 363 — argv-face inline callback on map / filter /
    // forEach: the collector reshaped it variadic, so its public
    // type is `(...args: any[]) => R` and the method-table arm
    // would reject it. See [`crate::check_type_of_call_arr_hof_argv_cb`].
    if let Some(r) =
        crate::check_type_of_call_arr_hof_argv_cb::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Array<T>.map(cb)` heterogeneous return — `(T) => U` for
    // primitive `U` (Number / String / Boolean / Any) answers
    // `Array<U>`. Homogeneous and Void-ret keep the two earlier
    // arms; see [`crate::check_type_of_call_arr_map_hetero`].
    if let Some(r) = crate::check_type_of_call_arr_map_hetero::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Array<T>.reduce(cb, seed)` heterogeneous acc — `(U, T) => R`
    // for primitive `U` != T answers `R`, checked against a
    // matching seed; see [`crate::check_type_of_call_arr_reduce_hetero`].
    if let Some(r) =
        crate::check_type_of_call_arr_reduce_hetero::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Array<T>.flatMap(cb)` heterogeneous cb return — `(T) => Array<U>`
    // for primitive `U` != T answers `Array<U>`; homogeneous
    // `(T) => T[]` keeps the method-table arm. See
    // [`crate::check_type_of_call_arr_flat_map_hetero`].
    if let Some(r) =
        crate::check_type_of_call_arr_flat_map_hetero::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Array<T>.flatMap(cb)` scalar cb return — `(T) => U` (primitive
    // U) answers `Array<U>` per ES §23.1.3.11 step 8.d (a non-Array
    // cb result acts like `[U]`). See
    // [`crate::check_type_of_call_arr_flat_map_scalar`].
    if let Some(r) =
        crate::check_type_of_call_arr_flat_map_scalar::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    None
}
