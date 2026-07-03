//! Cascade segment 4/6 of [`crate::check_type_of_call::check`]'s
//! early-route chain. Wedge order preserved verbatim from the
//! pre-split cascade; segment boundaries are mechanical
//! (consecutive), NOT semantic regroupings — relative order of
//! every arm is unchanged.
//!
//! Covers: string_trim_case / string_normalize_form / number_fixed_{0arg,trailing} /
//! array_concat / array_copy_within / string_pad / array_join /
//! string_slice_{0_1arg,2arg} / string_slicepad_trailing /
//! string_locale_compare / array_keys / value_of / array_pop_shift /
//! array_reverse_join / string_match / array_copy_within_fill

use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // S281 — `s.{trim,trimStart,trimEnd,trimLeft,trimRight,
    // toUpperCase,toLowerCase,toWellFormed,isWellFormed}(
    // ...trailing)` 0-arg trailing-arg ignore arm extracted
    // to [`crate::check_type_of_call_string_trim_case`]
    // (chunk 247).
    if let Some(r) =
        crate::check_type_of_call_string_trim_case::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 m1.h.48 — `s.normalize(form)` 1-arg optional-
    // form wedge arm extracted to
    // [`crate::check_type_of_call_string_normalize_form`]
    // (chunk 248).
    if let Some(r) =
        crate::check_type_of_call_string_normalize_form::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 m1.h.46 + S229 — `n.{toFixed,toExponential,
    // toPrecision}()` 0-arg or 1-arg-undefined arm
    // extracted to
    // [`crate::check_type_of_call_number_fixed_0arg`]
    // (chunk 249).
    if let Some(r) =
        crate::check_type_of_call_number_fixed_0arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S254 — `n.{toFixed,toExponential,toPrecision}(digits,
    // ...trailing)` trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_number_fixed_trailing`]
    // (chunk 250).
    if let Some(r) =
        crate::check_type_of_call_number_fixed_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 wedge — `xs.concat(...)` Array-receiver multi-
    // arg arm extracted to
    // [`crate::check_type_of_call_array_concat`]
    // (chunk 251).
    if let Some(r) = crate::check_type_of_call_array_concat::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // V3-18 + S219 + S335 — `xs.copyWithin(target[, start
    // [, end]])` Array-receiver 1-3-arg arm extracted to
    // [`crate::check_type_of_call_array_copy_within`]
    // (chunk 252).
    if let Some(r) =
        crate::check_type_of_call_array_copy_within::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // V3-18 m1.h.45 + S201 + S223 + S338 + S236 —
    // `s.{padStart,padEnd}(maxLength?[, fillStr?])` String-
    // receiver 0-2-arg arm extracted to
    // [`crate::check_type_of_call_string_pad`]
    // (chunk 253).
    if let Some(r) = crate::check_type_of_call_string_pad::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // V3-18 m1.h.42 + S206 — `xs.join()` / `xs.join(undefined)`
    // Array-receiver 0-arg / 1-arg-undef-sep wedge arm
    // extracted to
    // [`crate::check_type_of_call_array_join`]
    // (chunk 254).
    if let Some(r) = crate::check_type_of_call_array_join::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // V3-18 m1.h.36 + S232 + S333 —
    // `s.{slice,substring,substr}(start?)` String-receiver
    // 0-1-arg wedge arm extracted to
    // [`crate::check_type_of_call_string_slice_0_1arg`]
    // (chunk 255).
    if let Some(r) =
        crate::check_type_of_call_string_slice_0_1arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S221 + S232 + S333 — `s.{slice,substring,substr}(start,
    // end)` String-receiver 2-arg wedge arm extracted to
    // [`crate::check_type_of_call_string_slice_2arg`]
    // (chunk 256).
    if let Some(r) =
        crate::check_type_of_call_string_slice_2arg::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S241 + S284 — `s.{slice,substring,substr,padStart,
    // padEnd}(a, b, ...trailing)` String-receiver trailing-
    // arg ignore wedge arm extracted to
    // [`crate::check_type_of_call_string_slicepad_trailing`]
    // (chunk 257).
    if let Some(r) =
        crate::check_type_of_call_string_slicepad_trailing::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S211 + S238 + S285 — `s.localeCompare(thatStr,
    // ...trailing)` String-receiver 1+arg wedge arm
    // extracted to
    // [`crate::check_type_of_call_string_locale_compare`]
    // (chunk 258).
    if let Some(r) =
        crate::check_type_of_call_string_locale_compare::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S292 — `xs.keys(...trailing)` Array-receiver trailing-
    // arg ignore arm extracted to
    // [`crate::check_type_of_call_array_keys`] (chunk 259).
    if let Some(r) = crate::check_type_of_call_array_keys::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S290 — primitive `.valueOf(...trailing)` trailing-arg
    // ignore arm extracted to
    // [`crate::check_type_of_call_value_of`] (chunk 260).
    if let Some(r) = crate::check_type_of_call_value_of::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S288 — `xs.{pop,shift}(...trailing)` Array-receiver
    // trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_array_pop_shift`]
    // (chunk 261).
    if let Some(r) =
        crate::check_type_of_call_array_pop_shift::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S287 — `xs.{reverse,toReversed,join,toString,
    // toLocaleString}(...trailing)` Array-receiver trailing-
    // arg ignore arm extracted to
    // [`crate::check_type_of_call_array_reverse_join`]
    // (chunk 262).
    if let Some(r) =
        crate::check_type_of_call_array_reverse_join::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    // S286 — `s.{match,matchAll}(re, ...trailing)` String-
    // receiver trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_string_match`] (chunk 263).
    if let Some(r) = crate::check_type_of_call_string_match::try_match(checker, ast, callee, args) {
        return Some(r);
    }
    // S246 — `xs.{copyWithin,fill}(a, b, c, ...trailing)`
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_copy_within_fill`]
    // (chunk 264).
    if let Some(r) =
        crate::check_type_of_call_array_copy_within_fill::try_match(checker, ast, callee, args)
    {
        return Some(r);
    }
    None
}
