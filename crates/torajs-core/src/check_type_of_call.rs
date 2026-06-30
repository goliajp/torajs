//! `Expr::Call` typecheck extracted from
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Call` arm
//! (chunk 168).
//!
//! Pre-extract this arm was 4431 LOC inside `type_of_inner` — the
//! largest single arm in the type checker. Body verbatim moves here
//! as a `check` free fn taking `&mut Checker`; `type_of_inner`'s arm
//! delegates with one line. Same known-debt pattern as chunk 167's
//! Expr::Member extraction.
//!
//! Body unchanged — handles every call-site shape:
//! - cm_demote (class-method rewrite vs builtin-container)
//! - synthetic `__torajs_in_op` for binary `in` operator
//! - Promise<T>.then/catch/finally typing
//! - Array.from / Object.entries / Object.keys / Object.values
//! - All builtin global call shapes (parseInt / parseFloat /
//!   isNaN / Math.* / Number.* / String.* / JSON.* / Array.* /
//!   Object.* / Reflect.* / etc.)
//! - Generic fn-call typing with type-param resolution
//! - Class method dispatch
//! - Closure call
//! - Builtin string/array/number/bigint/regex/date/map/set/promise
//!   instance method dispatch
//! - Special cases (eval / Function ctor / Bun.* / process.* /
//!   console.* / fs.* / fetch / etc.)

#![allow(clippy::too_many_arguments)]

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{
    Checker, STRING_BORROW_METHODS, Type, is_class_method_name, struct_is_prefix_subtype,
};

pub(crate) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Result<Type, String> {
    // Name-based class-method rewrite vs builtin-container
    // receiver — decision + alt typecheck live in cm_demote.rs.
    if let Some(demoted) = checker.try_demote_cm_rewrite(ast, eid, args) {
        return demoted;
    }
    // T-45 — synthetic call from parser for binary `in`
    // operator: `__torajs_in_op(key, obj)`. Wedge extracted to
    // [`crate::check_type_of_call_in_op`] (chunk 296).
    if let Some(r) = crate::check_type_of_call_in_op::try_match(checker, ast, callee, args) {
        return r;
    }
    // Promise<T>.then / .catch early-route arms (T-19.l 2-arg
    // shape, T-19.o heterogeneous T→U + P10.7 Promise<Any>,
    // P10.2-A1.1 Promise<Undefined>, P10.2-A4 Promise<Array<U>>)
    // — see [`crate::check_type_of_call_promise_then`] (chunk
    // 207 — first sub-batch of check_type_of_call.rs per-shape
    // decomposition). All 4 patterns must run BEFORE the
    // regular method-table dispatch because the table's
    // static signature fixes arg count and inner-T constraint.
    // `.finally` is intentionally not handled — its cb is
    // `() => void` and the table arm already covers it.
    if let Some(r) = crate::check_type_of_call_promise_then::try_match(checker, ast, callee, args) {
        return r;
    }
    // Global bare-Ident ctor / coercion call shapes
    // (`fetch(url)` / `Number|String|Boolean(x)` callable
    // coercion / `BigInt(value)` ctor / `Symbol(desc?)`) —
    // see [`crate::check_type_of_call_global_ctors`] (chunk
    // 208 — second sub-batch of check_type_of_call.rs per-
    // shape decomposition). All 4 are early-route Ident
    // callee shapes that must run BEFORE the regular
    // method-table / general-call dispatch.
    if let Some(r) = crate::check_type_of_call_global_ctors::try_match(checker, ast, callee, args) {
        return r;
    }
    // Number.parseInt + Number.parseFloat early-route arms —
    // see [`crate::check_type_of_call_number_parse`] (chunk
    // 209 — third sub-batch). Both need early-route handling
    // because the regular static-method table fixes arity in
    // ways the spec ignores (parseInt 1-arg, parseFloat 0-arg
    // both had to circumvent the unified arity gate).
    if let Some(r) = crate::check_type_of_call_number_parse::try_match(checker, ast, callee, args) {
        return r;
    }
    // T-15.g.5 / T-19.b/d/f — `Promise.resolve(v)` /
    // `Promise.reject(v)` with arg-type-driven return
    // inference. Extracted to `check/promise_static.rs`
    // (2026-06-03, P10.5-A2 prereq).
    if let Some(r) = checker.check_promise_resolve_reject_static(ast, *callee, args) {
        return r;
    }
    // P10.5-A4 — `process.on('unhandledRejection', cb)`.
    // Extracted to `check/process_on.rs`.
    if let Some(r) = checker.check_process_on(ast, *callee, args) {
        return r;
    }
    // Promise.all / .race / .any / .allSettled fan-in static
    // methods — see [`crate::check_type_of_call_promise_all`]
    // (chunk 210 — fourth sub-batch). Input is
    // Array<Promise<T>>; result varies per method
    // (.all → Promise<T[]> / .race | .any → Promise<T> /
    // .allSettled → Promise<{status,value}[]>).
    if let Some(r) = crate::check_type_of_call_promise_all::try_match(checker, ast, callee, args) {
        return r;
    }
    // Object.assign / Object.values static-method early-route
    // arms — see [`crate::check_type_of_call_object_static`]
    // (chunk 211 — fifth sub-batch). Object.assign requires
    // target+sources identical struct types in this subset;
    // Object.values is polymorphic over Array / String / Any /
    // struct receivers.
    if let Some(r) = crate::check_type_of_call_object_static::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `arr.flat(N)` literal-depth early-route arm — see
    // [`crate::check_type_of_call_arr_flat`] (chunk 212 —
    // sixth sub-batch). Peels `Array<>` layers from the
    // receiver's element type when the depth arg is a
    // Number / `Infinity` / `undefined` literal. The 0-arg
    // `xs.flat()` shape uses the regular method-table arm.
    if let Some(r) = crate::check_type_of_call_arr_flat::try_match(checker, ast, callee, args) {
        return r;
    }
    // Array.from(iter, mapFn?) polymorphic early-route arm —
    // see [`crate::check_type_of_call_array_from`] (chunk 213
    // — seventh sub-batch). 1-arg receiver-polymorphic over
    // String / Array / Set; 2+ arg `Array.from(iter, mapFn,
    // thisArg?)` result is Array<mapFn ret>.
    if let Some(r) = crate::check_type_of_call_array_from::try_match(checker, ast, callee, args) {
        return r;
    }
    // S153 — `Date.UTC(...)` 1-6 arg overload early-route arm —
    // see [`crate::check_type_of_call_date_utc`] (chunk 214 —
    // eighth sub-batch). Per ES §21.4.2.21 trailing-defaults
    // overloads; the 7-arg form keeps using the static-sig path
    // unchanged.
    if let Some(r) = crate::check_type_of_call_date_utc::try_match(checker, ast, callee, args) {
        return r;
    }
    // `xs.reduce(cb)` / `xs.reduceRight(cb)` 1-arg overload
    // early-route arm — see
    // [`crate::check_type_of_call_reduce_1arg`] (chunk 215 —
    // ninth sub-batch). ES §23.1.3.24 / §23.1.3.25 init-value-
    // defaulting form; the 2-arg form is covered by the
    // static-sig arm.
    if let Some(r) = crate::check_type_of_call_reduce_1arg::try_match(checker, ast, callee, args) {
        return r;
    }
    // M3 — generic call inference for bare-Ident callee
    // naming a generic FnDecl — see
    // [`crate::check_type_of_call_generic_ident`] (chunk 219
    // — thirteenth sub-batch). T-28 trailing-typevar-Any
    // padding + regular unify path; side-table records the
    // inferred substitution so ssa_lower can monomorphize.
    if let Some(r) =
        crate::check_type_of_call_generic_ident::try_match(checker, ast, eid, callee, args)
    {
        return r;
    }
    // `console.{log,error,warn,info,debug}(...)` varargs-
    // widening arm — see [`crate::check_type_of_call_console`]
    // (chunk 216 — tenth sub-batch). S328 WHATWG console
    // §1.1.{2,4}; widens past the fixed-arity Type::Function
    // sig so any arg count is acceptable.
    if let Some(r) = crate::check_type_of_call_console::try_match(checker, ast, callee, args) {
        return r;
    }
    // `JSON.stringify(value, replacer?, indent?)` varargs-
    // widening arm — see
    // [`crate::check_type_of_call_json_stringify`] (chunk 217
    // — eleventh sub-batch). S311 ES §25.5.2 silent trailing-
    // arg ignore; runtime keeps consuming only the indent
    // shape for now.
    if let Some(r) = crate::check_type_of_call_json_stringify::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `n.toString(radix?)` primitive arms (BigInt + Number
    // receivers) — see
    // [`crate::check_type_of_call_to_string_radix`] (chunk
    // 218 — twelfth sub-batch). S247 BigInt trailing-arg
    // ignore + S229/S244 Number radix + trailing-arg handling.
    if let Some(r) =
        crate::check_type_of_call_to_string_radix::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // (`Number(x)` / `String(x)` callable coercion is fully
    // covered by `check_type_of_call_global_ctors` chunk 208 —
    // the Number|String|Boolean arm there is more permissive
    // (S251 trailing-arg ignore + Boolean) and fires earlier
    // in this cascade, so the previously-duplicated narrow
    // arm here was dead code; removed chunk 220.)

    // Bare-Ident JS globals dispatch
    // (parseInt / parseFloat / isNaN / isFinite /
    // queueMicrotask) — see
    // [`crate::check_type_of_call_bare_globals`] (chunk 221
    // — fourteenth sub-batch). Each branch returns the
    // spec-aligned result type after typechecking arg shape;
    // global isNaN / isFinite coerce via ToNumber (vs the
    // strict Number.X namespaced methods).
    if let Some(r) = crate::check_type_of_call_bare_globals::try_match(checker, ast, callee, args) {
        return r;
    }
    // Math.hypot variadic arm — see
    // [`crate::check_type_of_call_math_hypot`] (chunk 222
    // — fifteenth sub-batch). Spec §21.3.2.18; widens past
    // the fixed-arity Type::Function sig. S271 accepts
    // Undefined alongside Number per ToNumber(undefined)=NaN.
    if let Some(r) = crate::check_type_of_call_math_hypot::try_match(checker, ast, callee, args) {
        return r;
    }
    // Explicit-Undefined / Any 1-arg spec-corner widen arms
    // (Date.parse / Math.<unary> / String.fromCharCode /
    // String.fromCodePoint) — see
    // [`crate::check_type_of_call_undef_widen_1arg`] (chunk
    // 223 — sixteenth sub-batch). Each pattern widens past
    // the static-sig rejection of explicit Undefined / Any
    // 1-arg so ssa_lower can fold the spec-aligned result.
    if let Some(r) =
        crate::check_type_of_call_undef_widen_1arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // (Math.<unary>(undefined) handled by chunk 223 sibling.)
    // S231 — `String.fromCharCode(undefined)` per ES §22.1.2.1:
    // each arg is converted via ToUint16; ToUint16(undefined) = 0,
    // so the single-arg undef shape yields " ". The standard
    // Type::Function arm (vec![Type::Number]) rejects the typed
    // Undefined operand with "argument 0: expected Number, got
    // Undefined"; widen here so ssa_lower's mirror substitutes
    // ConstI64(0) into the helper call. fromCodePoint diverges
    // here — bun throws a RangeError at runtime for undefined,
    // so its throw-shape alignment stays L3b.
    // (String.fromCharCode 1-arg undef/Any handled by chunk 223 sibling.)
    // S340 — `String.fromCodePoint(Any)` per ES §22.1.2.2 step 2:
    // ToNumber accepts arbitrary-typed input; RangeError throw
    // shape (non-integer / out-of-range [0, 0x10FFFF]) is
    // enforced by the runtime helper `str_from_code_point`
    // (pending throw + emit_throw_check propagates), so the
    // Any path inherits the same throw semantics for free.
    // Sister to S329 (fromCharCode Any).
    // (String.fromCodePoint 1-arg Any handled by chunk 223 sibling.)
    // `String.fromCharCode(...codes)` — variadic. Each code is a
    // Number; result is a String. The single-arg case still goes
    // through the general type table for the intrinsic call; we
    // only intercept when the arity is ≠ 1.
    // `Array.isArray()` 0-arg + `Array.of(...vals)` variadic
    // arms — see
    // [`crate::check_type_of_call_array_isarray_of`] (chunk
    // 224 — seventeenth sub-batch). S204 isArray 0-arg
    // statically-false predicate (sig table rejects no-arg) +
    // Array.of variadic factory anchoring element type on
    // first arg.
    if let Some(r) =
        crate::check_type_of_call_array_isarray_of::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `String.fromCharCode(...)` + `String.fromCodePoint(...)`
    // variadic arms (arity ≠ 1) — see
    // [`crate::check_type_of_call_string_from_var`] (chunk
    // 225 — eighteenth sub-batch). S340 variadic Any per ES
    // §22.1.2.{1,2} step 2; the arity-1 case stays on the
    // general Type::Function table for the intrinsic call.
    if let Some(r) =
        crate::check_type_of_call_string_from_var::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `s.concat(...others)` arms — see
    // [`crate::check_type_of_call_string_concat`] (chunk
    // 226 — nineteenth sub-batch). arity ≠ 1 variadic +
    // S212 arity-1 typed-Undefined widen, both gated on
    // Type::String receiver. The arity-1 non-Undefined case
    // stays on the general Type::Function table.
    if let Some(r) = crate::check_type_of_call_string_concat::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S203 — `Math.<unary>()` 0-arg arms — see
    // [`crate::check_type_of_call_math_unary_0arg`] (chunk
    // 227 — twentieth sub-batch). Accept the no-arg form
    // rejected by `vec![Type::Number]` at the generic arity
    // gate; ssa_lower emits the static NaN return.
    if let Some(r) =
        crate::check_type_of_call_math_unary_0arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `Math.{pow,atan2,imul}` short-arity (S205) + 2-arg
    // undef widen (S228) arms — see
    // [`crate::check_type_of_call_math_pow_atan2_imul`]
    // (chunk 228 — twenty-first sub-batch). Disjoint from
    // `Math.{min,max}` arm just below; placing the sibling
    // here preserves cascade semantics (B can never match
    // pow/atan2/imul).
    if let Some(r) =
        crate::check_type_of_call_math_pow_atan2_imul::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `Math.{min,max}(...)` variadic arm — see
    // [`crate::check_type_of_call_math_min_max`] (chunk 229
    // — twenty-second sub-batch). V3-18 m1.h.24 drops the
    // artificial 2-arg minimum; S228 Undefined + S342 Any
    // arg propagation.
    if let Some(r) = crate::check_type_of_call_math_min_max::try_match(checker, ast, callee, args) {
        return r;
    }
    // `xs.slice([start [, end]])` 0/1/2-arg Array-receiver
    // arm — see [`crate::check_type_of_call_array_slice`]
    // (chunk 230 — twenty-third sub-batch). V3-18 m1.h.35
    // widens past the pre-fix 2-fixed-param declaration so
    // 0/1-arg + S213 typed-Undefined + S334 Any all fall to
    // the same ssa_lower default-filling path.
    if let Some(r) = crate::check_type_of_call_array_slice::try_match(checker, ast, callee, args) {
        return r;
    }
    // `xs.fill(v [, start [, end]])` 1/2/3-arg Array-receiver
    // arm — see [`crate::check_type_of_call_array_fill`]
    // (chunk 231 — twenty-fourth sub-batch). V3-18 m1.h.53
    // widens past pre-fix 3-fixed-param sig; S218 typed-
    // Undefined start/end + S335 Any start/end.
    if let Some(r) = crate::check_type_of_call_array_fill::try_match(checker, ast, callee, args) {
        return r;
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
        return r;
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
        return r;
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
        return r;
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
        return r;
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
        return r;
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
        return r;
    }
    // V3-18 wedge — `xs.{sort,toSorted}(cmp?, ...trailing)`
    // 0/1/2+-arg Array-receiver arm — see
    // [`crate::check_type_of_call_array_sort`] (chunk 238 —
    // thirty-first sub-batch). Folds 0-arg default, S303 1-arg
    // Undefined-cmp, and S276 2+-arg trailing-ignore into one
    // sibling; 1-arg non-Undefined falls through to the regular
    // comparator-type-check path.
    if let Some(r) = crate::check_type_of_call_array_sort::try_match(checker, ast, callee, args) {
        return r;
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
        return r;
    }
    // S225 — `xs.at(idx)` 1-arg Array-receiver arm extracted
    // to [`crate::check_type_of_call_array_at_1arg`] (chunk
    // 240). Accepts `Undefined` idx per ES §23.1.3.1
    // step 2-3 (`ToIntegerOrInfinity(undefined) = 0`).
    if let Some(r) = crate::check_type_of_call_array_at_1arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S342 — `Math.<unary/binary>(Any[, Any])` widen arm
    // extracted to [`crate::check_type_of_call_math_any_widen`]
    // (chunk 242). Returns `Some(Ok(Number))` only when at
    // least one Any arg is seen; falls through to the strict
    // method-table when all args are Number.
    if let Some(r) = crate::check_type_of_call_math_any_widen::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 wedge / S202 / S253 — `Number.{isFinite,isNaN,
    // isInteger,isSafeInteger}(value, ...trailing)` loose
    // predicate arm extracted to
    // [`crate::check_type_of_call_number_predicate_loose`]
    // (chunk 241).
    if let Some(r) =
        crate::check_type_of_call_number_predicate_loose::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 wedge — `s.{charAt,charCodeAt,codePointAt}()`
    // 0-arg String-receiver arm extracted to
    // [`crate::check_type_of_call_string_char_0arg`]
    // (chunk 243).
    if let Some(r) =
        crate::check_type_of_call_string_char_0arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S222 — `s.{at,charAt,charCodeAt,codePointAt}(undefined)`
    // 1-arg String-receiver undef-index arm extracted to
    // [`crate::check_type_of_call_string_char_undef`]
    // (chunk 244).
    if let Some(r) =
        crate::check_type_of_call_string_char_undef::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S332 — `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)`
    // 1-arg String-receiver Any-widen arm extracted to
    // [`crate::check_type_of_call_string_char_any`]
    // (chunk 245).
    if let Some(r) =
        crate::check_type_of_call_string_char_any::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S240 — `s.{at,charAt,charCodeAt,codePointAt,repeat,
    // normalize}(useful, ...trailing)` trailing-arg ignore
    // arm extracted to
    // [`crate::check_type_of_call_string_trailing_ignore`]
    // (chunk 246).
    if let Some(r) =
        crate::check_type_of_call_string_trailing_ignore::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S281 — `s.{trim,trimStart,trimEnd,trimLeft,trimRight,
    // toUpperCase,toLowerCase,toWellFormed,isWellFormed}(
    // ...trailing)` 0-arg trailing-arg ignore arm extracted
    // to [`crate::check_type_of_call_string_trim_case`]
    // (chunk 247).
    if let Some(r) =
        crate::check_type_of_call_string_trim_case::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 m1.h.48 — `s.normalize(form)` 1-arg optional-
    // form wedge arm extracted to
    // [`crate::check_type_of_call_string_normalize_form`]
    // (chunk 248).
    if let Some(r) =
        crate::check_type_of_call_string_normalize_form::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 m1.h.46 + S229 — `n.{toFixed,toExponential,
    // toPrecision}()` 0-arg or 1-arg-undefined arm
    // extracted to
    // [`crate::check_type_of_call_number_fixed_0arg`]
    // (chunk 249).
    if let Some(r) =
        crate::check_type_of_call_number_fixed_0arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S254 — `n.{toFixed,toExponential,toPrecision}(digits,
    // ...trailing)` trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_number_fixed_trailing`]
    // (chunk 250).
    if let Some(r) =
        crate::check_type_of_call_number_fixed_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 wedge — `xs.concat(...)` Array-receiver multi-
    // arg arm extracted to
    // [`crate::check_type_of_call_array_concat`]
    // (chunk 251).
    if let Some(r) = crate::check_type_of_call_array_concat::try_match(checker, ast, callee, args) {
        return r;
    }
    // V3-18 + S219 + S335 — `xs.copyWithin(target[, start
    // [, end]])` Array-receiver 1-3-arg arm extracted to
    // [`crate::check_type_of_call_array_copy_within`]
    // (chunk 252).
    if let Some(r) =
        crate::check_type_of_call_array_copy_within::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // V3-18 m1.h.45 + S201 + S223 + S338 + S236 —
    // `s.{padStart,padEnd}(maxLength?[, fillStr?])` String-
    // receiver 0-2-arg arm extracted to
    // [`crate::check_type_of_call_string_pad`]
    // (chunk 253).
    if let Some(r) = crate::check_type_of_call_string_pad::try_match(checker, ast, callee, args) {
        return r;
    }
    // V3-18 m1.h.42 + S206 — `xs.join()` / `xs.join(undefined)`
    // Array-receiver 0-arg / 1-arg-undef-sep wedge arm
    // extracted to
    // [`crate::check_type_of_call_array_join`]
    // (chunk 254).
    if let Some(r) = crate::check_type_of_call_array_join::try_match(checker, ast, callee, args) {
        return r;
    }
    // V3-18 m1.h.36 + S232 + S333 —
    // `s.{slice,substring,substr}(start?)` String-receiver
    // 0-1-arg wedge arm extracted to
    // [`crate::check_type_of_call_string_slice_0_1arg`]
    // (chunk 255).
    if let Some(r) =
        crate::check_type_of_call_string_slice_0_1arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S221 + S232 + S333 — `s.{slice,substring,substr}(start,
    // end)` String-receiver 2-arg wedge arm extracted to
    // [`crate::check_type_of_call_string_slice_2arg`]
    // (chunk 256).
    if let Some(r) =
        crate::check_type_of_call_string_slice_2arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S241 + S284 — `s.{slice,substring,substr,padStart,
    // padEnd}(a, b, ...trailing)` String-receiver trailing-
    // arg ignore wedge arm extracted to
    // [`crate::check_type_of_call_string_slicepad_trailing`]
    // (chunk 257).
    if let Some(r) =
        crate::check_type_of_call_string_slicepad_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S211 + S238 + S285 — `s.localeCompare(thatStr,
    // ...trailing)` String-receiver 1+arg wedge arm
    // extracted to
    // [`crate::check_type_of_call_string_locale_compare`]
    // (chunk 258).
    if let Some(r) =
        crate::check_type_of_call_string_locale_compare::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S292 — `xs.keys(...trailing)` Array-receiver trailing-
    // arg ignore arm extracted to
    // [`crate::check_type_of_call_array_keys`] (chunk 259).
    if let Some(r) = crate::check_type_of_call_array_keys::try_match(checker, ast, callee, args) {
        return r;
    }
    // S290 — primitive `.valueOf(...trailing)` trailing-arg
    // ignore arm extracted to
    // [`crate::check_type_of_call_value_of`] (chunk 260).
    if let Some(r) = crate::check_type_of_call_value_of::try_match(checker, ast, callee, args) {
        return r;
    }
    // S288 — `xs.{pop,shift}(...trailing)` Array-receiver
    // trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_array_pop_shift`]
    // (chunk 261).
    if let Some(r) =
        crate::check_type_of_call_array_pop_shift::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S287 — `xs.{reverse,toReversed,join,toString,
    // toLocaleString}(...trailing)` Array-receiver trailing-
    // arg ignore arm extracted to
    // [`crate::check_type_of_call_array_reverse_join`]
    // (chunk 262).
    if let Some(r) =
        crate::check_type_of_call_array_reverse_join::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S286 — `s.{match,matchAll}(re, ...trailing)` String-
    // receiver trailing-arg ignore arm extracted to
    // [`crate::check_type_of_call_string_match`] (chunk 263).
    if let Some(r) = crate::check_type_of_call_string_match::try_match(checker, ast, callee, args) {
        return r;
    }
    // S246 — `xs.{copyWithin,fill}(a, b, c, ...trailing)`
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_copy_within_fill`]
    // (chunk 264).
    if let Some(r) =
        crate::check_type_of_call_array_copy_within_fill::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S245/S276 — `Array<T>.{reduce,reduceRight}(fn, init,
    // ...trailing)` Array-receiver trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_array_reduce_trailing`]
    // (chunk 295).
    if let Some(r) =
        crate::check_type_of_call_array_reduce_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S242/S299 — `arr.{at,slice,join}(useful, ...trailing)`
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_at_slice_join`]
    // (chunk 294).
    if let Some(r) =
        crate::check_type_of_call_array_at_slice_join::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S239 — String/Array.{indexOf,lastIndexOf,includes}
    // + String.{startsWith,endsWith}(needle, fromIndex,
    // ...trailing) trailing-arg ignore per ES
    // §22.1.3.{8,10,5,21,7} / §23.1.3.{14,17,18}: spec
    // reserves slots after fromIndex but tora's helpers
    // are 2-arg only. Trim trailing operands at lower-time
    // (ssa_lower mirrors break early past i=1 / drop
    // args[2..]). Same shape as S238 localeCompare.
    //
    // S278 — `recv.{indexOf,lastIndexOf,includes,startsWith,
    // endsWith}(needle, fromIndex, ...trailing)` String- +
    // Array-receiver trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_index_search_trailing`]
    // (chunk 265).
    if let Some(r) =
        crate::check_type_of_call_index_search_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `Object.{keys,getOwnPropertyNames}(obj, ...trailing)` /
    // `Reflect.ownKeys(obj, ...trailing)` namespace
    // 1-useful-arg trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_keys_ownkeys`]
    // (chunk 293).
    if let Some(r) =
        crate::check_type_of_call_object_keys_ownkeys::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // `Object.{entries,freeze,isFrozen,values}(obj,
    // ...trailing)` Object-namespace 1-useful-arg trailing-
    // arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_entries_freeze`]
    // (chunk 292).
    if let Some(r) =
        crate::check_type_of_call_object_entries_freeze::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S270/S271/S272 — `Object.{hasOwn,is}(target, key,
    // ...trailing)` / `Reflect.{has,get}(target, key,
    // ...trailing)` namespace 3+arg trailing-arg ignore
    // wedge extracted to
    // [`crate::check_type_of_call_object_reflect_3arg`]
    // (chunk 291).
    if let Some(r) =
        crate::check_type_of_call_object_reflect_3arg::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // Boolean / Symbol / String primitive `toString` /
    // `toLocaleString` trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_primitive_proto_trailing`]
    // (chunk 290).
    if let Some(r) =
        crate::check_type_of_call_primitive_proto_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S304 — Struct-instance Object.prototype methods
    // trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_struct_proto_trailing`]
    // (chunk 289).
    if let Some(r) =
        crate::check_type_of_call_struct_proto_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S261 — Date instance 0-arg getter / format method
    // family trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_date_instance_trailing`]
    // (chunk 288).
    if let Some(r) =
        crate::check_type_of_call_date_instance_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S260 — `n.toLocaleString(locales?, options?, ...trailing)`
    // Number-receiver 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_number_tolocale_trailing`]
    // (chunk 287).
    if let Some(r) =
        crate::check_type_of_call_number_tolocale_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S259 — `Symbol.{for,keyFor}(key, ...trailing)`
    // namespace trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_symbol_static_trailing`]
    // (chunk 286).
    if let Some(r) =
        crate::check_type_of_call_symbol_static_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S248 — `Set.add(value, ...trailing)` / `Map.set(key,
    // value, ...trailing)` Set/Map-receiver trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_mapset_add_set`]
    // (chunk 266).
    if let Some(r) = crate::check_type_of_call_mapset_add_set::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S269 + S317 — `Object.{create,setPrototypeOf,
    // defineProperties,defineProperty}(...)` Object-namespace
    // trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_object_static_proto`]
    // (chunk 267).
    if let Some(r) =
        crate::check_type_of_call_object_static_proto::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S268 — `d.set{FullYear,Month,Date,Hours,Minutes,
    // Seconds,Milliseconds,Time,Year}(..., ...trailing)`
    // Date-instance setter trailing-arg ignore wedge
    // extracted to [`crate::check_type_of_call_date_setter`]
    // (chunk 268).
    if let Some(r) = crate::check_type_of_call_date_setter::try_match(checker, ast, callee, args) {
        return r;
    }
    // S267 — `Array.isArray(value, ...trailing)` Array-
    // namespace trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_array_isarray_trailing`]
    // (chunk 269).
    if let Some(r) =
        crate::check_type_of_call_array_isarray_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S266 — `r.{test,exec,toString}(s?, ...trailing)`
    // RegExp-receiver trailing-arg ignore wedge extracted
    // to [`crate::check_type_of_call_regexp_test_exec`]
    // (chunk 270).
    if let Some(r) =
        crate::check_type_of_call_regexp_test_exec::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S265 — `Object.{getPrototypeOf,isExtensible,isSealed,
    // preventExtensions,seal}(obj, ...trailing)` Object-
    // namespace meta-method trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_object_static_meta`]
    // (chunk 271).
    if let Some(r) =
        crate::check_type_of_call_object_static_meta::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S264 — `(Set|Map).{has,delete}(key, ...trailing)` /
    // `Map.get(key, ...trailing)` / `(Set|Map).clear(...trailing)`
    // Set/Map-receiver instance-method trailing-arg ignore
    // wedge extracted to
    // [`crate::check_type_of_call_mapset_query`]
    // (chunk 272).
    if let Some(r) = crate::check_type_of_call_mapset_query::try_match(checker, ast, callee, args) {
        return r;
    }
    // S301 — `WeakMap.{set,get,has,delete}` /
    // `WeakSet.{add,has,delete}` WeakMap/WeakSet-receiver
    // instance-method trailing-arg ignore wedge extracted to
    // [`crate::check_type_of_call_weak_collection`]
    // (chunk 273).
    if let Some(r) =
        crate::check_type_of_call_weak_collection::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S318 — `Set.{isSubsetOf,isSupersetOf,isDisjointFrom,
    // union,intersection,difference,symmetricDifference}
    // (other, ...trailing)` Set ES2025 setops trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_set_ops`]
    // (chunk 274).
    if let Some(r) = crate::check_type_of_call_set_ops::try_match(checker, ast, callee, args) {
        return r;
    }
    // S315 — `Object.getOwnPropertyDescriptor(obj, key,
    // ...trailing)` Object-namespace 2-arg-spec trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_object_getownpropdesc`]
    // (chunk 275).
    if let Some(r) =
        crate::check_type_of_call_object_getownpropdesc::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S314 — `BigInt.{asIntN,asUintN}(bits, value,
    // ...trailing)` BigInt-namespace 2-arg-spec trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_bigint_asint`]
    // (chunk 276).
    if let Some(r) = crate::check_type_of_call_bigint_asint::try_match(checker, ast, callee, args) {
        return r;
    }
    // S309 — `Object.fromEntries(entries, ...trailing)`
    // Object-namespace 1-arg-spec trailing-arg ignore wedge
    // extracted to
    // [`crate::check_type_of_call_object_fromentries`]
    // (chunk 277).
    if let Some(r) =
        crate::check_type_of_call_object_fromentries::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S210 — `String.search()` / `String.search(undefined)`
    // short-circuit wedge extracted to
    // [`crate::check_type_of_call_string_search_short_circuit`]
    // (chunk 278).
    if let Some(r) =
        crate::check_type_of_call_string_search_short_circuit::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S325 — `WeakRef.deref(...trailing)` trailing-arg
    // ignore wedge extracted to
    // [`crate::check_type_of_call_weakref_deref`]
    // (chunk 279).
    if let Some(r) = crate::check_type_of_call_weakref_deref::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S324 — `String.search(needle, ...trailing)` trailing-
    // arg ignore wedge extracted to
    // [`crate::check_type_of_call_string_search_trailing`]
    // (chunk 280).
    if let Some(r) =
        crate::check_type_of_call_string_search_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S209 — `String.repeat(undefined)` 1-arg-undef-count
    // widen wedge extracted to
    // [`crate::check_type_of_call_string_repeat_undef`]
    // (chunk 281).
    if let Some(r) =
        crate::check_type_of_call_string_repeat_undef::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S207 — `String.replace` / `String.replaceAll` with
    // fewer-than-2-arg widen wedge extracted to
    // [`crate::check_type_of_call_string_replace_undef`]
    // (chunk 282).
    if let Some(r) =
        crate::check_type_of_call_string_replace_undef::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S339 — `xs.with(Any idx, val)` Any-index widen wedge
    // extracted to [`crate::check_type_of_call_array_with_any`]
    // (chunk 283).
    if let Some(r) = crate::check_type_of_call_array_with_any::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S283 — Array.prototype.with(index, value, ...trailing)
    // 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_array_with_trailing`]
    // (chunk 284).
    if let Some(r) =
        crate::check_type_of_call_array_with_trailing::try_match(checker, ast, callee, args)
    {
        return r;
    }
    // S282 — String.{replace,replaceAll,split}(useful, useful,
    // ...trailing) 3+ arg trailing-arg wedge extracted to
    // [`crate::check_type_of_call_string_replace_split_trailing`]
    // (chunk 285).
    if let Some(r) = crate::check_type_of_call_string_replace_split_trailing::try_match(
        checker, ast, callee, args,
    ) {
        return r;
    }
    let callee_ty = checker.type_of(ast, *callee)?;
    let Type::Function(mut params, ret) = callee_ty else {
        return Err(format!("not callable: type {callee_ty:?}"));
    };
    // P1 wedge — Array.prototype callback methods accept
    // an optional trailing thisArg per ES spec §23.1.3.X
    // (map/filter/every/some/forEach/find/findIndex/
    // findLast/findLastIndex/reduce/reduceRight/flatMap).
    // tora's callbacks don't have `this` semantics
    // (closures don't bind a receiver), so the thisArg
    // is silently dropped — tests that don't rely on
    // `this` inside the callback now typecheck (~70+
    // cases unblocked across the broader sample). Tests
    // that DO use `this` were already blocked on the
    // missing-this substrate; the silent drop doesn't
    // make those worse.
    let mut effective_args = args.clone();
    // P1 / S270 — Array.prototype callback methods trailing
    // thisArg drop wedge extracted to
    // [`crate::check_type_of_call_p1_thisarg`] (chunk 297).
    crate::check_type_of_call_p1_thisarg::apply(
        checker,
        ast,
        callee,
        params.len(),
        &mut effective_args,
    )?;
    // T-28 — Default param missing → undefined (per ES
    // spec §10.2.1.4). When fewer args are supplied than
    // params, JS sets the missing slots to undefined. Only
    // safe for Type::Any params (typed slots can't hold
    // undefined). Typed missing params still error so
    // typed code keeps strict arity. ssa_lower pads the
    // missing positions with ANY_UNDEF boxes at the call
    // site.
    if effective_args.len() < params.len() {
        let trailing_all_any = params[effective_args.len()..]
            .iter()
            .all(|t| matches!(t, Type::Any));
        if trailing_all_any {
            // Type-check what was actually passed (rest stay
            // as undefined). Pad-with-undef happens at SSA
            // layer via the `padded_args` path keyed off
            // expr_arity_pad. Stash the missing count on
            // the call site so ssa_lower can emit ANY_UNDEF
            // boxes for the trailing positions.
            for arg_id in effective_args.iter() {
                let _ = checker.type_of(ast, *arg_id)?;
            }
            checker
                .arity_pad_count
                .insert(eid, params.len() - effective_args.len());
            return Ok((*ret).clone());
        }
    }
    // Date per-field setters (setFullYear / setMonth /
    // setHours / setMinutes / setSeconds / …) accept 1-N
    // args per ES §21.4.4.20-26 with trailing positions
    // optional. The sig declares the FULL arity (max); when
    // the caller supplied fewer args we narrow the sig to
    // the supplied arity so strict-arity below passes. The
    // ssa_lower side sentinel-pads the missing trailing
    // positions with `DATE_FIELD_KEEP` (i64::MIN).
    if effective_args.len() < params.len()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && matches!(
            name.as_str(),
            "setFullYear" | "setMonth" | "setHours" | "setMinutes" | "setSeconds"
        )
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        if recv_ty == Type::Date && effective_args.len() >= 1 {
            params.truncate(effective_args.len());
        }
    }
    // `arr.splice(start, deleteCount?)` — per ES §23.1.3.31
    // both args are spec-optional: 0-arg form gives
    // `start = 0`/`deleteCount = 0`, 1-arg form defaults
    // `deleteCount = len - actualStart`. The Type::Function
    // declared above is the 2-arg full form; truncate
    // params to the actual arity so the strict-equality
    // arity check below accepts 0 / 1 args. The
    // `ssa_lower_splice` siblings fill the defaults at
    // emit time (0 → `ConstI64(0)`, 1 → `i64::MAX`
    // sentinel that the helper clamps to `len - start`).
    if effective_args.len() < params.len()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && matches!(name.as_str(), "splice" | "toSpliced")
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        if matches!(recv_ty, Type::Array(_)) {
            params.truncate(effective_args.len());
        }
    }
    // S237 — `arr.splice(start, undefined)` /
    // `arr.toSpliced(start, undefined)` per ES §23.1.3.31
    // step 7: ToIntegerOrInfinity(undefined) = 0, so an
    // explicit-undefined deleteCount yields 0 removal
    // (matches bun: returns `[]`, leaves the source
    // unchanged). Distinct from the omitted-arg 1-arg
    // shape above which spec-defaults deleteCount to
    // `len - actualStart`. Trim both `params` and
    // `effective_args` to the 1-arg shape when arg 1 is
    // Undefined so the strict-equality arity check below
    // accepts it; ssa_lower_splice mirror detects the
    // 2-arg-undef shape and emits ConstI64(0) for
    // deleteCount instead of the 1-arg i64::MAX sentinel.
    if effective_args.len() == 2
        && params.len() == 2
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && matches!(name.as_str(), "splice" | "toSpliced")
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        let arg1_ty = checker.type_of(ast, effective_args[1])?;
        if matches!(recv_ty, Type::Array(_)) && matches!(arg1_ty, Type::Undefined) {
            params.truncate(1);
            effective_args.truncate(1);
        }
    }
    // `arr.at(index?)` / `s.at(index?)` — per ES §22.1.3.1 /
    // §23.1.3.1 step 2-3, `undefined` routes through
    // ToIntegerOrInfinity → 0, so the 0-arg form returns
    // index 0. The Type::Function declared above is the
    // 1-arg full form; truncate to 0 params on no-arg call
    // so the strict-equality arity check accepts it. The
    // `ssa_lower_str` Arr and String `at` emitters fill the
    // default `ConstI64(0)` at emit time.
    if effective_args.len() < params.len()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
        && name == "at"
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        if matches!(recv_ty, Type::Array(_) | Type::String) {
            params.truncate(effective_args.len());
        }
    }
    // `arr.indexOf() / .includes() / .lastIndexOf()` and
    // `s.indexOf() / .includes() / .lastIndexOf() /
    // .startsWith() / .endsWith()` — per ES §22.1.3.{8,13,...}
    // and §23.1.3.{14,17,18}, `searchElement` / `searchString`
    // defaults to undefined when omitted (algorithm steps
    // read the missing argument as undefined). The
    // Type::Function declared above is the 1-arg full form;
    // truncate to 0 params on the no-arg call.
    //
    // SSA emit semantics:
    //   String: fill needle = ToString(undefined) = "undefined"
    //     and search normally (bun observable).
    //   Array<T> with T ≠ Any: undefined cannot strict-equal
    //     any typed element, so the result is fixed
    //     (-1 / false). Array<Any> is omitted from the
    //     truncate here because the search would need an
    //     Any-tagged undefined sentinel; L3b.
    if effective_args.is_empty()
        && let Expr::Member { obj, name } = ast.get_expr(*callee)
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        let trunc = match (&recv_ty, name.as_str()) {
            (Type::String, "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith") => {
                true
            }
            (Type::Array(elem), "indexOf" | "includes" | "lastIndexOf") => **elem != Type::Any,
            _ => false,
        };
        if trunc {
            params.truncate(0);
        }
    }
    // S243 — narrow trailing-arg ignore for Math.* namespace
    // methods per ES §21.3.2.* "trailing args are ignored":
    // each Math.* algorithm reads only the declared positional
    // args; the surface Type::Function sig is closed but the
    // underlying calling convention is open-arity. SSA-emit's
    // generic Math.* call lowering already truncates extras
    // before the (f64, f64...) / (f64,) helper ABI, so we
    // only need to truncate at the check arity gate and
    // type_of the trailing operands for side effects. Other
    // builtin receivers (Number/Array/String) stay on the
    // existing narrow carve-outs (S238–S242) until per-
    // method SSA-emit shape widens to match.
    if effective_args.len() > params.len()
        && let Expr::Member { obj, name: _ } = ast.get_expr(*callee)
    {
        let recv_ty = checker.type_of(ast, *obj)?;
        // S243 Math.* / S250 Date.<static> narrow trailing-
        // arg ignore per ES §21.3.2.* / §21.4.3.*. Each
        // intrinsic reads only declared positional args;
        // SSA-emit's Call lowering tolerates extras at the
        // (intrinsic) ABI boundary. Other builtin receivers
        // (Number/Array/String) stay on per-method narrow
        // carve-outs (S238-S248).
        if matches!(recv_ty, Type::Object("Math") | Type::Object("Date")) {
            for &aid in &effective_args[params.len()..] {
                let _ = checker.type_of(ast, aid)?;
            }
            effective_args.truncate(params.len());
        }
    }
    if params.len() != effective_args.len() {
        return Err(format!(
            "expected {} argument(s), got {}",
            params.len(),
            effective_args.len()
        ));
    }
    let args = &effective_args;
    // M6.1 — String borrow-methods (slice/includes/indexOf/...)
    // don't transfer ownership of either receiver or args.
    // They read both, allocate a fresh result, and return.
    let is_string_borrow = matches!(
        ast.get_expr(*callee),
        Expr::Member { obj: _, name }
            if STRING_BORROW_METHODS.iter().any(|m| *m == name.as_str())
    );
    // M5.1 — class methods (`__cm_C__m(receiver, ...)`) borrow
    // the receiver: arg[0] is read, never consumed. Args[1..]
    // follow the normal affine rules.
    let is_class_method = matches!(
        ast.get_expr(*callee),
        Expr::Ident(name) if is_class_method_name(name)
    );
    // Per-call-site consume bitmap, derived from
    // `ast.consuming_params` for the callee fn (computed by
    // `compute_consuming_params` from the body's flow into
    // `__new_*` / `this.<field> =` sinks). For unknown
    // callees (intrinsics, builtins) the default is "borrow"
    // — only the constructor-factory shortcut here triggers
    // when consuming_params doesn't have an entry.
    let consume_bitmap: Vec<bool> = match ast.get_expr(*callee) {
        Expr::Ident(callee_name) => {
            if let Some(bm) = ast.consuming_params.get(callee_name) {
                bm.clone()
            } else if callee_name.starts_with("__new_") {
                vec![true; args.len()]
            } else {
                vec![false; args.len()]
            }
        }
        _ => vec![false; args.len()],
    };
    for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
        let arg_ty = checker.type_of(ast, *arg_id)?;
        // M5.2 — class-method dispatch: arg[0] is the receiver
        // and may be a SUBCLASS of the declared param type
        // (structural super-set: subclass struct's fields are
        // a prefix-extension of the parent's). The SSA / LLVM
        // layer treats both as ptr, so the call is correct as
        // long as the layout prefix matches. We just skip the
        // strict equality here.
        let skip_type_check =
            is_class_method && i == 0 && struct_is_prefix_subtype(&arg_ty, param_ty);
        // V3-18 wedge — Nullable<T> param accepts both
        // T-typed and Null arg (TS spec §3.9.2.4 optional
        // param widens to T | undefined; subset models
        // optional as Nullable<T>).
        let nullable_match = if let Type::Nullable(inner) = param_ty {
            arg_ty == Type::Null || &arg_ty == inner.as_ref()
        } else {
            false
        };
        // S133 narrow — callback Function subtype:
        // JS spec lets a callback accept fewer args than
        // the formal Function param declares
        // (Map.forEach: `(v) =>` legal even though spec sig
        // is `(v, k, map) => void`). Strict equality on
        // Type::Function rejects shorter callbacks. Accept
        // when actual arity ≤ formal arity, every prefix
        // slot matches with either side being Any (user
        // callback without type-ann defaults to Any —
        // accept against typed formal; formal Any accepts
        // any typed actual), and the return type matches
        // (or either side Any).
        let callback_subtype = match (param_ty, &arg_ty) {
            (Type::Function(formal_ps, formal_ret), Type::Function(actual_ps, actual_ret)) => {
                actual_ps.len() <= formal_ps.len()
                    && (formal_ret.as_ref() == actual_ret.as_ref()
                        || matches!(formal_ret.as_ref(), Type::Any)
                        || matches!(actual_ret.as_ref(), Type::Any))
                    && actual_ps
                        .iter()
                        .zip(formal_ps.iter())
                        .all(|(a, f)| a == f || matches!(f, Type::Any) || matches!(a, Type::Any))
            }
            _ => false,
        };
        if !skip_type_check
            && !nullable_match
            && !callback_subtype
            && param_ty != &Type::Any
            && &arg_ty != param_ty
        {
            return Err(format!(
                "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
            ));
        }
        // TS-shape: function parameters borrow non-Copy args
        // by default. Calling `f(x)` does not mark `x` as
        // moved — the caller keeps owning the heap and can
        // pass the same binding to another function later.
        // Matches JS pass-by-reference semantics. Caveat: a
        // function that stores its arg into long-lived heap
        // (e.g. a global, or a returned struct field) would
        // create a dangling pointer once the caller drops
        // the original — there's no GC to keep it alive. For
        // the cases we ship today this is fine; the ts-subset
        // doc calls out the constraint.
        let _ = is_string_borrow;
        let _ = is_class_method;
        if consume_bitmap.get(i).copied().unwrap_or(false)
            && !arg_ty.is_copy()
            && !checker.consumed_calls.contains(&eid)
        {
            checker.consume(ast, *arg_id);
        }
    }
    checker.consumed_calls.insert(eid);
    Ok(*ret)
}
