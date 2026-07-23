//! Cascade segment 3/6 of [`crate::check_type_of_call::check`]'s
//! early-route chain. Wedge order preserved verbatim from the
//! pre-split cascade; segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings — relative order of
//! every arm is unchanged.
//!
//! Covers: array_slice / array_fill / string_predicate_2arg / string_search_undef /
//! string_index_2arg / array_push_unshift / string_split_2arg /
//! mapset_iter_trailing / array_sort / array_index_2arg / array_at_1arg /
//! math_any_widen / number_predicate_loose / string_char_{0arg,undef,any} /
//! string_trailing_ignore

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

// CARVE-OUT: dispatch table — 21 mechanically-ordered `try_match` wedges
// (arm order preserved verbatim from chunk 433 pre-split cascade + rotation
// 159 Any-needle admit + splice-insert knife 2 + groupBy Array-items lane
// wedges appended). Refactor blocked by wedge order being semantically
// load-bearing — an earlier wedge pre-empts a later, more general shape;
// arms cannot be regrouped without perturbing acceptance. Grows one line
// per new wedge.
pub(crate) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // `xs.slice([start [, end]])` 0/1/2-arg Array-receiver
    // arm — see [`crate::check_type_of_call_array_slice`]
    // (chunk 230 — twenty-third sub-batch). V3-18 m1.h.35
    // widens past the pre-fix 2-fixed-param declaration so
    // 0/1-arg + S213 typed-Undefined + S334 Any all fall to
    // the same ssa_lower default-filling path.
    if let Some(r) = crate::check_type_of_call_array_slice::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // `xs.fill(v [, start [, end]])` 1/2/3-arg Array-receiver
    // arm — see [`crate::check_type_of_call_array_fill`]
    // (chunk 231 — twenty-fourth sub-batch). V3-18 m1.h.53
    // widens past pre-fix 3-fixed-param sig; S218 typed-
    // Undefined start/end + S335 Any start/end.
    if let Some(r) = crate::check_type_of_call_array_fill::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // `String.{startsWith,endsWith,includes}(needle, pos?)`
    // 2-arg arm — see
    // [`crate::check_type_of_call_string_predicate_2arg`]
    // (chunk 232 — twenty-fifth sub-batch). V3-18 m1.h.51
    // widens past pre-fix 1-fixed-param sig; S224 accepts
    // Undefined position slot.
    if let Some(r) =
        crate::check_type_of_call_string_predicate_2arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S235 — `String.{indexOf,lastIndexOf,includes,
    // startsWith,endsWith,search}(undefined)` 1-arg-undef-
    // needle widen — see
    // [`crate::check_type_of_call_string_search_undef`]
    // (chunk 233 — twenty-sixth sub-batch). ssa_lower_str's
    // `undef_to_str_at_arg0` mirror substitutes the interned
    // "undefined" literal for the helper's (Str, Str) ABI.
    if let Some(r) =
        crate::check_type_of_call_string_search_undef::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 m1.h.50 — `String.{indexOf,lastIndexOf}(needle,
    // fromIndex?)` 2-arg widen + S214/S216 Undefined fromIndex
    // arm — see
    // [`crate::check_type_of_call_string_index_2arg`] (chunk
    // 234 — twenty-seventh sub-batch). ssa_lower mirror lowers
    // `undef` → `ConstI64(0)` for indexOf and `ConstI64(i64::MAX)`
    // for lastIndexOf (NaN → +∞ per ES §22.1.3.10 step 5);
    // helper clamps `from > len` to `len`.
    if let Some(r) =
        crate::check_type_of_call_string_index_2arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // Rotation 159 — `xs.{indexOf,lastIndexOf,includes}(anyNeedle,
    // from?)` Any-needle admit over typed-element receivers — see
    // [`crate::check_type_of_call_index_search_any`]. Paired with
    // `emit_compare`'s needle-Any arm (element packs as the pair
    // against the boxed needle; includes rides SameValueZero).
    if let Some(r) =
        crate::check_type_of_call_index_search_any::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // RFC 20260720-splice-insert knife 2 — `xs.splice(start, dc,
    // ...items)` 3+-arg insert arm — see
    // [`crate::check_type_of_call_array_splice_insert`]. Items match
    // the elem type (Any admits into Number/String elems, paired
    // with coerce_push_value's unbox); result is the removed
    // Array<T>.
    if let Some(r) =
        crate::check_type_of_call_array_splice_insert::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge — `xs.{push,unshift}(...vals)` variadic
    // Array-receiver arm — see
    // [`crate::check_type_of_call_array_push_unshift`] (chunk
    // 235 — twenty-eighth sub-batch). Each arg is appended (or
    // prepended) in order; subset typecheck enforces element-
    // type compatibility and returns Void (push's new-length
    // return is not surfaced).
    if let Some(r) =
        crate::check_type_of_call_array_push_unshift::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge — `s.split(sep, limit?)` 2-arg arm + S215
    // Undefined limit widen — see
    // [`crate::check_type_of_call_string_split_2arg`] (chunk
    // 236 — twenty-ninth sub-batch). ssa_lower mirror inline-
    // replaces `argv[2]` with `ConstI64(i64::MAX)` for the
    // Undefined limit case so the take-min branch falls to `len`.
    if let Some(r) =
        crate::check_type_of_call_string_split_2arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S277 — `Map.{keys,values,entries}(...trailing)` /
    // `Set.{keys,values,entries}(...trailing)` iterator-factory
    // trailing-arg-ignore arm — see
    // [`crate::check_type_of_call_mapset_iter_trailing`] (chunk
    // 237 — thirtieth sub-batch). Spec defines 0-arg only;
    // typecheck-and-drop trailing exprs.
    if let Some(r) =
        crate::check_type_of_call_mapset_iter_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge — `xs.{sort,toSorted}(cmp?, ...trailing)`
    // 0/1/2+-arg Array-receiver arm — see
    // [`crate::check_type_of_call_array_sort`] (chunk 238 —
    // thirty-first sub-batch). Folds 0-arg default, S303 1-arg
    // Undefined-cmp, and S276 2+-arg trailing-ignore into one
    // sibling; 1-arg non-Undefined falls through to the regular
    // comparator-type-check path.
    if let Some(r) = crate::check_type_of_call_array_sort::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // V3-18 m1.h.49 — `xs.{indexOf,lastIndexOf,includes}
    // (needle, fromIndex)` 2-arg Array-receiver arm — see
    // [`crate::check_type_of_call_array_index_2arg`] (chunk
    // 239 — thirty-second sub-batch). S127-4 Array<Any>
    // cross-type needle + S217 Undefined fromIndex + S331 Any
    // fromIndex per ES §22.1.3.{13,14,16}.
    if let Some(r) =
        crate::check_type_of_call_array_index_2arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S225 — `xs.at(idx)` 1-arg Array-receiver arm extracted
    // to [`crate::check_type_of_call_array_at_1arg`] (chunk
    // 240). Accepts `Undefined` idx per ES §23.1.3.1
    // step 2-3 (`ToIntegerOrInfinity(undefined) = 0`).
    if let Some(r) = crate::check_type_of_call_array_at_1arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S342 — `Math.<unary/binary>(Any[, Any])` widen arm
    // extracted to [`crate::check_type_of_call_math_any_widen`]
    // (chunk 242). Returns `Some(Ok(Number))` only when at
    // least one Any arg is seen; falls through to the strict
    // method-table when all args are Number.
    if let Some(r) = crate::check_type_of_call_math_any_widen::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge / S202 / S253 — `Number.{isFinite,isNaN,
    // isInteger,isSafeInteger}(value, ...trailing)` loose
    // predicate arm extracted to
    // [`crate::check_type_of_call_number_predicate_loose`]
    // (chunk 241).
    if let Some(r) =
        crate::check_type_of_call_number_predicate_loose::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge — `s.{charAt,charCodeAt,codePointAt}()`
    // 0-arg String-receiver arm extracted to
    // [`crate::check_type_of_call_string_char_0arg`]
    // (chunk 243).
    if let Some(r) =
        crate::check_type_of_call_string_char_0arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S222 — `s.{at,charAt,charCodeAt,codePointAt}(undefined)`
    // 1-arg String-receiver undef-index arm extracted to
    // [`crate::check_type_of_call_string_char_undef`]
    // (chunk 244).
    if let Some(r) =
        crate::check_type_of_call_string_char_undef::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S332 — `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)`
    // 1-arg String-receiver Any-widen arm extracted to
    // [`crate::check_type_of_call_string_char_any`]
    // (chunk 245).
    if let Some(r) =
        crate::check_type_of_call_string_char_any::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S240 — `s.{at,charAt,charCodeAt,codePointAt,repeat,
    // normalize}(useful, ...trailing)` trailing-arg ignore
    // arm extracted to
    // [`crate::check_type_of_call_string_trailing_ignore`]
    // (chunk 246).
    if let Some(r) =
        crate::check_type_of_call_string_trailing_ignore::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `String.raw(template, ...substitutions)` per ES §22.1.2.4
    // — direct-fn call shape; kernel `__torajs_string_raw` walks
    // the template's `raw` array interleaved with substitutions.
    // See [`crate::check_type_of_call_string_raw`].
    if let Some(r) = crate::check_type_of_call_string_raw::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // `Object.groupBy(items, callbackFn)` per ES §20.1.2.10 —
    // Array items lane (iterable-only receivers are L3b). Kernel
    // `__torajs_object_group_by` walks the array and dispatches
    // the cb via the uniform any-call ABI.
    // See [`crate::check_type_of_call_object_group_by`].
    if let Some(r) =
        crate::check_type_of_call_object_group_by::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // `Map.groupBy(items, callbackFn)` per ES §24.2.2.4 — sister
    // wedge to Object.groupBy; accumulator is a Map with
    // SameValueZero keys (no ToPropertyKey coercion).
    // See [`crate::check_type_of_call_map_group_by`].
    if let Some(r) = crate::check_type_of_call_map_group_by::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    None
}
