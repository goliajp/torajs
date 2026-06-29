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
    // operator: `__torajs_in_op(key, obj)`. ssa_lower
    // intercepts by name and emits the type-dispatched
    // membership check. Returns Boolean unconditionally.
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == "__torajs_in_op"
        && args.len() == 2
    {
        let _ = checker.type_of(ast, args[0])?;
        let obj_ty = checker.type_of(ast, args[1])?;
        if !matches!(obj_ty, Type::Array(_) | Type::Any | Type::Struct(_)) {
            return Err(format!(
                "`in` rhs must be Array, Struct, or any (subset stub); got {obj_ty:?}"
            ));
        }
        return Ok(Type::Boolean);
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
    // S235 — String.{indexOf,lastIndexOf,includes,startsWith,
    // endsWith,search}(undefined) 1-arg-undef-needle per ES
    // §22.1.3.{8,10,5,21,7,16} step 1-3: ToString(undefined)
    // = "undefined". The Type::Function arm declares the
    // needle as String and rejects the typed-Undefined
    // operand. Widen the 1-arg dispatch; ssa_lower_str's
    // `undef_to_str_at_arg0` mirror substitutes the interned
    // "undefined" literal for the helper's (Str, Str) ABI.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith" | "search"
        )
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let needle_ty = checker.type_of(ast, args[0])?;
            if matches!(needle_ty, Type::Undefined) {
                return Ok(
                    if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith") {
                        Type::Boolean
                    } else {
                        Type::Number
                    },
                );
            }
        }
    }
    // V3-18 m1.h.50 — String.indexOf / lastIndexOf accept
    // an optional 2nd `fromIndex` arg per JS spec §21.1.3.7
    // / §21.1.3.10. Pre-fix tora declared with 1 fixed
    // param.
    //
    // S214 — String.indexOf(needle, undefined) per ES
    // §22.1.3.8 step 4: ToIntegerOrInfinity(undefined)=0,
    // accept Undefined fromIndex and treat as 0. ssa_lower
    // mirror inline-replaces argv[1] with ConstI64(0).
    // (lastIndexOf NaN→+∞ default is a follow-up ship.)
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "indexOf" | "lastIndexOf")
        && args.len() == 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let needle_ty = checker.type_of(ast, args[0])?;
            if !matches!(needle_ty, Type::String) {
                return Err(format!(
                    "String.{m_name} arg 0 must be string, got {needle_ty:?}"
                ));
            }
            let from_ty = checker.type_of(ast, args[1])?;
            let from_undef = matches!(from_ty, Type::Undefined);
            // S214 indexOf → fromIndex=0; S216 lastIndexOf
            // → fromIndex=+∞ (ES §22.1.3.10 step 5: NaN→+∞)
            // → ssa_lower lowers to i64::MAX and the helper
            // clamps `from > len` to len, matching spec.
            let allow_undef = from_undef;
            if from_ty != Type::Number && !allow_undef {
                return Err(format!(
                    "String.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                ));
            }
            return Ok(Type::Number);
        }
    }
    // V3-18 wedge — Array.push / Array.unshift accept
    // a variable number of args per JS spec §22.1.3.20
    // / §22.1.3.34. Each arg is appended (or prepended)
    // in order. Pre-fix tora's strict 1-arg signature
    // rejected the multi-arg form. Subset typecheck
    // enforces every arg matches the element type and
    // returns Void (push's new-length return is not
    // surfaced).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "push" | "unshift")
        && args.len() != 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = src_ty {
            let inner = (*elem).clone();
            for (i, aid) in args.iter().enumerate() {
                let aty = checker.type_of(ast, *aid)?;
                if aty != inner && aty != Type::Any {
                    return Err(format!(
                        "Array.{m_name} arg {i}: expected element type {:?}, got {aty:?}",
                        inner
                    ));
                }
            }
            return Ok(Type::Void);
        }
    }
    // V3-18 wedge — String.split accepts an optional
    // 2nd `limit` arg per JS spec §22.1.3.21. Returns
    // first `limit` substrings (or fewer if the source
    // splits into fewer). Pre-fix tora's strict 1-arg
    // signature rejected the 2-arg form.
    //
    // S215 — `s.split(sep, undefined)` per ES §22.1.3.21
    // step 2: `If limit is undefined, lim = 2^32-1` (no
    // truncation). Accept Undefined limit; ssa_lower
    // mirror inline-replaces argv[2] with ConstI64(i64::MAX)
    // so the take-min branch falls to len.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "split"
        && args.len() == 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let _ = checker.type_of(ast, args[0])?;
            let limit_ty = checker.type_of(ast, args[1])?;
            if limit_ty != Type::Number && limit_ty != Type::Undefined {
                return Err(format!(
                    "String.split arg 1 (limit) must be number, got {limit_ty:?}"
                ));
            }
            return Ok(Type::Array(Box::new(Type::String)));
        }
    }
    // V3-18 wedge — Array.sort / toSorted accept an
    // optional comparator. Per JS spec §22.1.3.27 the
    // default cmp converts to string and compares
    // lexicographically; subset uses element-type-aware
    // `<`/`>` comparison via the runtime helper. Pre-fix
    // tora's strict 1-arg signature rejected the no-arg
    // form `arr.sort()`.
    //
    // ES §23.1.3.{29,31} step 1 also explicitly accepts
    // the literal `undefined` for `comparefn`: "If
    // comparefn is not undefined and not callable, throw
    // a TypeError". bun observes `undefined` → default
    // compare; tora's strict comparator-type check
    // rejected the 1-arg form `.sort(undefined)`. Treat
    // a 1-arg call with arg type Undefined as equivalent
    // to the no-arg case (SSA mirror at
    // ssa_lower_str.rs `sort/toSorted` dispatch).
    // S277 — Map/Set.{keys,values,entries}(...trailing) per
    // ES §23.{1,2}.3.X iterator-factory trailing-arg ignore.
    // Spec defines 0-arg only; trailing slots silent-drop.
    // tora's static sig is `Vec::new() → MapIter`; 1+ args
    // bounce at strict arity. Accept any arg count and
    // typecheck-and-drop trailing exprs.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "keys" | "values" | "entries")
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Map | Type::Set) {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::MapIter);
        }
    }
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "sort" | "toSorted")
    {
        if args.is_empty() {
            let src_ty = checker.type_of(ast, *src_id)?;
            if let Type::Array(elem) = src_ty {
                return Ok(Type::Array(elem));
            }
        } else if args.len() == 1 {
            let arg_ty = checker.type_of(ast, args[0])?;
            if arg_ty == Type::Undefined {
                let src_ty = checker.type_of(ast, *src_id)?;
                if let Type::Array(elem) = src_ty {
                    return Ok(Type::Array(elem));
                }
            }
        } else if args.len() >= 2 {
            // S276 — Array.{sort,toSorted}(cmp, ...trailing) per
            // ES §23.1.3.{30,33} trailing-arg ignore: spec
            // reads only cmp; trailing slots silent-drop.
            // tora's static sig is fixed 1-arg, so 2+ args
            // bounce at strict arity. Accept any cmp shape
            // (Function | Any), typecheck-and-drop trailing.
            // S303 — also accept Undefined cmp per ES §23.1.3.30
            // step 2: `sort(undef)` = `sort()` (default lex
            // comparator). The 1-arg `undef` branch above
            // handles args.len()==1; this widens 2+ to fold
            // through the same default path + drop trailing.
            let src_ty = checker.type_of(ast, *src_id)?;
            if let Type::Array(elem) = src_ty {
                let aty0 = checker.type_of(ast, args[0])?;
                if matches!(aty0, Type::Function(..) | Type::Any | Type::Undefined) {
                    for &a in &args[1..] {
                        let _ = checker.type_of(ast, a)?;
                    }
                    return Ok(Type::Array(elem));
                }
            }
        }
    }
    // V3-18 m1.h.49 — Array.indexOf / lastIndexOf accept
    // an optional fromIndex 2nd arg per JS spec §22.1.3.13
    // / §22.1.3.16. Pre-fix tora declared with 1 fixed
    // param so 2-arg calls hit the arity check.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
        && args.len() == 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let needle_ty = checker.type_of(ast, args[0])?;
            // S127-4: Array<Any> accepts cross-type needle —
            // ssa-lower's strict-eq packing arm already
            // handles I64/F64/Bool/Ptr/refcounted/Any
            // (ssa_lower_str.rs §arr.indexOf needle pack).
            // The 1-arg path falls through the generic arg-
            // unify which skips equality when param is Any,
            // but this dedicated 2-arg branch hand-wrote a
            // strict-eq compare with no Any escape. Bring it
            // in line with the 1-arg case + spec §22.1.3.x
            // (no static type restriction on needle).
            if needle_ty != **elem && !matches!(**elem, Type::Any) {
                return Err(format!(
                    "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
                    **elem
                ));
            }
            let from_ty = checker.type_of(ast, args[1])?;
            // S217 — Array.{indexOf,lastIndexOf,includes}
            // (needle, undefined) per ES §22.1.3.{13,14,16}:
            // ToIntegerOrInfinity(undefined)=0. Accept
            // Undefined alongside Number; ssa_lower mirror
            // short-circuits fromIndex=undefined to
            // ConstI64(0).
            //
            // S331 — widen accept Any fromIndex per ES
            // §23.1.3.{14,17,18} step 4: ToIntegerOrInfinity
            // already coerces arbitrary-typed input (NaN→0,
            // ±∞→sat). ssa_lower mirror routes Any through
            // anyv_to_number → coerce_to_i64. Pattern A
            // sister to S327 / S329.
            if from_ty != Type::Number && from_ty != Type::Undefined && from_ty != Type::Any {
                return Err(format!(
                    "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                ));
            }
            return Ok(if m_name == "includes" {
                Type::Boolean
            } else {
                Type::Number
            });
        }
    }
    // S225 — typed Array<T>.at(undefined) per ES §23.1.3.1
    // step 2-3: ToIntegerOrInfinity(undefined)=0, returns
    // arr[0]. Accept Undefined alongside Number; ssa_lower
    // mirror short-circuits idx=undefined to ConstI64(0)
    // before coerce_to_i64 (matches S222 charAt early-
    // intercept idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "at"
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let idx_ty = checker.type_of(ast, args[0])?;
            if !matches!(idx_ty, Type::Number | Type::Undefined) {
                return Err(format!("Array.at arg 0 must be number, got {idx_ty:?}"));
            }
            return Ok((**elem).clone());
        }
    }
    // V3-18 wedge — Number.isFinite / isNaN / isInteger /
    // isSafeInteger per JS spec §21.1.2.2 / §21.1.2.4 /
    // §21.1.2.3 / §21.1.2.5: these methods do NOT coerce
    // their argument. They return true iff the arg is a
    // Number value AND satisfies the finite / NaN /
    // integer / safe-integer predicate; for non-Number
    // args (string / boolean / null / object / array)
    // they return false statically. The existing
    // signature `(Number) -> Boolean` rejects non-Number
    // args with a type error, but that's wrong for spec
    // and breaks the canonical TS feature-detection
    // idiom `if (Number.isFinite(maybeStringy)) ...`.
    // S342 — `Math.<unary/binary>(Any[, Any])` per ES §21.3.2.*:
    // every Math method that takes Number args applies ToNumber,
    // which accepts arbitrary-typed input. Method-table sigs
    // `vec![Type::Number]` / `vec![Type::Number, Type::Number]`
    // at the Math dispatch site (~2912 / ~2943) strict-rejected
    // `o: any` operands. Widen here so the ssa_lower mirror
    // routes Any through anyv_to_number → F64 → Math helper.
    // Per-arg check: Any or Number passes; everything else
    // (String, Bool, Object) still hits the strict gate.
    // min/max variadic stays one carve-out — the iter loop
    // covers 0..N args uniformly.
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Math"
        && matches!(
            m_name.as_str(),
            "sqrt"
                | "abs"
                | "floor"
                | "ceil"
                | "log"
                | "exp"
                | "sign"
                | "round"
                | "trunc"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "log2"
                | "log10"
                | "cbrt"
                | "sinh"
                | "cosh"
                | "tanh"
                | "asinh"
                | "acosh"
                | "atanh"
                | "expm1"
                | "log1p"
                | "clz32"
                | "fround"
                | "f16round"
                | "pow"
                | "min"
                | "max"
                | "atan2"
        )
    {
        let mut any_seen = false;
        let mut all_ok = true;
        for &aid in args {
            let aty = checker.type_of(ast, aid)?;
            if matches!(aty, Type::Any) {
                any_seen = true;
            } else if aty != Type::Number {
                all_ok = false;
                break;
            }
        }
        if any_seen && all_ok {
            return Ok(Type::Number);
        }
    }
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Number"
        && matches!(
            m_name.as_str(),
            "isFinite" | "isNaN" | "isInteger" | "isSafeInteger"
        )
    {
        // Force type_of on the arg so any internal
        // typecheck error still surfaces, but we don't
        // require it to be Number — non-Number args
        // route through the lower's static-false path.
        //
        // S202 — extend the same loose check to the 0-arg
        // form per §21.1.2.{3,5,7}: non-Number args
        // (including the implicit undefined) statically
        // return false; ssa_lower's short-circuit emits
        // ConstBool(false) without dispatching the helper.
        //
        // S253 — `Number.{isFinite,isNaN,isInteger,
        // isSafeInteger}(value, ...trailing)` trailing-arg
        // ignore per ES §21.1.2.{2,3,4,5}: spec reads only
        // args[0]; tora silent-drops trailing per generic
        // trailing-arg-ignore policy. SSA-emit short-
        // circuits non-Number args (ConstBool(false)) and
        // dispatches the helper for Number args, both
        // reading only args[0].
        if let Some(arg0) = args.first() {
            let _ = checker.type_of(ast, *arg0)?;
        }
        for &arg in args.iter().skip(1) {
            let _ = checker.type_of(ast, arg)?;
        }
        return Ok(Type::Boolean);
    }
    // V3-18 wedge — String.charAt / charCodeAt /
    // codePointAt accept an optional pos arg per JS
    // spec §22.1.3.4 / §22.1.3.5 / §22.1.3.6: missing
    // pos defaults to 0. Pre-fix tora declared with one
    // required param so 0-arg calls bounced at the
    // unified arity check with 'expected 1 argument(s),
    // got 0'. Implementation: typecheck-only pass through
    // for the missing-arg shape; ssa_lower's 1-arg path
    // gets a synthetic ConstI64(0) padded in for the
    // default.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "charAt" | "charCodeAt" | "codePointAt")
        && args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            return Ok(if m_name == "charAt" {
                Type::String
            } else {
                Type::Number
            });
        }
    }
    // S222 — `s.{at,charAt,charCodeAt,codePointAt}(undefined)`
    // per ES §22.1.3.{1,2,3,4} step 2-3:
    // ToIntegerOrInfinity(undefined)=0, so an explicit undef
    // index behaves as the 0-arg / index=0 form. ssa_lower
    // mirror short-circuits the Undefined slot to ConstI64(0).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "at" | "charAt" | "charCodeAt" | "codePointAt"
        )
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty = checker.type_of(ast, args[0])?;
            if matches!(aty, Type::Undefined) {
                return Ok(if matches!(m_name.as_str(), "charAt" | "at") {
                    Type::String
                } else {
                    Type::Number
                });
            }
        }
    }
    // S332 — `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)`
    // per ES §22.1.3.{1,2,3,4,17} step 2-3 (or step 1 for
    // repeat): ToIntegerOrInfinity accepts arbitrary-typed
    // input. The method-table sig `(Number) -> X` rejected
    // explicit `o: any` operands at typecheck; widen here
    // so the ssa_lower mirror routes Any through
    // anyv_to_number → coerce_to_i64 → helper. Sister to
    // S329 (fromCharCode Any).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat"
        )
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty = checker.type_of(ast, args[0])?;
            if matches!(aty, Type::Any) {
                return Ok(if matches!(m_name.as_str(), "charAt" | "at" | "repeat") {
                    Type::String
                } else {
                    Type::Number
                });
            }
        }
    }
    // S240 — String.{at,charAt,charCodeAt,codePointAt,
    // repeat,normalize}(useful, ...trailing) trailing-arg
    // ignore per ES §22.1.3.{1,2,3,4,17,13}: spec reserves
    // slots past useful arg 0 but tora's helpers are 1-arg
    // only. Trailing operands type_of'd for side effects;
    // ssa_lower mirror evals-and-drops past i=0.
    //
    // S272 widens from `args.len() == 2` (single trailing)
    // to `args.len() >= 2` (any trailing count).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" | "normalize"
        )
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty0 = checker.type_of(ast, args[0])?;
            let arg0_ok = match m_name.as_str() {
                "at" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" => {
                    matches!(aty0, Type::Number | Type::Undefined)
                }
                "normalize" => matches!(aty0, Type::String | Type::Undefined),
                _ => false,
            };
            if arg0_ok {
                for &a in &args[1..] {
                    let _ = checker.type_of(ast, a)?;
                }
                return Ok(match m_name.as_str() {
                    "at" | "charAt" | "repeat" | "normalize" => Type::String,
                    _ => Type::Number,
                });
            }
        }
    }
    // S281 — String.{trim,trimStart,trimEnd,trimLeft,
    // trimRight,toUpperCase,toLowerCase,toWellFormed,
    // isWellFormed}(...trailing) trailing-arg ignore per
    // ES §22.1.3.{30,32,33,28,29,34,10}. Spec-defined
    // 0-arg methods; spec reserves no positional slots so
    // any trailing operand is silently ignored. tora's
    // helpers are 0-arg only (recv only); typecheck-and-
    // drop trailing here; ssa_lower mirror lowers each
    // operand for side effects then drops the value (S272
    // idiom — never push into argv). toLocale{Upper,Lower}Case
    // is excluded (S140 sig + drop_args path own those —
    // they take a locales arg and already silent-drop it).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "trim"
                | "trimStart"
                | "trimEnd"
                | "trimLeft"
                | "trimRight"
                | "toUpperCase"
                | "toLowerCase"
                | "toWellFormed"
                | "isWellFormed"
        )
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            for &a in args {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(if m_name == "isWellFormed" {
                Type::Boolean
            } else {
                Type::String
            });
        }
    }
    // V3-18 m1.h.48 — String.normalize accepts an optional
    // form arg ("NFC" / "NFD" / "NFKC" / "NFKD"). Per JS
    // spec §21.1.3.13. tora's byte-Str ASCII-only path
    // returns identity for any form, so we just typecheck
    // and route through the existing 0-arg lowering.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "normalize"
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty = checker.type_of(ast, args[0])?;
            // S208 — spec §22.1.3.13 step 1: when `form` is
            // undefined, default to "NFC". Accept the typed-
            // Undefined arg shape alongside the String form;
            // ssa_lower routes both to the existing 0-arg
            // NFC-default path.
            if matches!(aty, Type::Undefined) {
                return Ok(Type::String);
            }
            if !matches!(aty, Type::String) {
                return Err(format!("String.normalize arg must be string, got {aty:?}"));
            }
            return Ok(Type::String);
        }
    }
    // V3-18 m1.h.46 — Number.toFixed / toExponential /
    // toPrecision with 0 args. Per JS spec §21.1.3.3 etc:
    //   n.toFixed()        defaults to digits = 0
    //   n.toExponential()  defaults to fractionDigits = "as
    //                       few as needed" (we use 6 — bun
    //                       matches; actual spec call ToInteger
    //                       on undefined gives 0 but bun's
    //                       output uses default precision)
    //   n.toPrecision()    no precision = same as toString
    // Pre-fix tora declared with 1 fixed param so 0-arg
    // calls failed at the arity check. Implementation:
    // typecheck-only pass through; ssa_lower handles the
    // missing-arg defaults.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision")
        && (args.is_empty()
            || (args.len() == 1 && matches!(checker.type_of(ast, args[0])?, Type::Undefined)))
    {
        // S229 — accept the 1-arg explicit-Undefined shape per
        // ES §21.1.3.{3,5,6} where an undef digits/precision
        // arg folds to the same default as the 0-arg form.
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Number) {
            return Ok(Type::String);
        }
    }
    // S254 — `n.{toFixed,toExponential,toPrecision}(digits,
    // ...trailing)` per ES §21.1.3.{3,5,6} trailing-arg
    // ignore. Spec reads only args[0] (digits/fractionDigits
    // /precision); tora silent-drops trailing per generic
    // trailing-arg-ignore policy. SSA-emit's per-method
    // dispatch (line ~16700) pushes args into argv; the
    // ssa-lower S254 mirror caps the push at args[0].
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "toFixed" | "toExponential" | "toPrecision")
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Number) {
            let arg0_ty = checker.type_of(ast, args[0])?;
            if !matches!(arg0_ty, Type::Number | Type::Undefined) {
                return Err(format!(
                    "Number.{m_name} arg 0 must be number, got {arg0_ty:?}"
                ));
            }
            for &arg in args.iter().skip(1) {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::String);
        }
    }
    // V3-18 wedge — Array.concat accepts any number of
    // array args per JS spec §22.1.3.2:
    //   xs.concat()            → fresh shallow copy of xs
    //   xs.concat(a, b, ..., z)→ fresh array of xs then a's
    //                             then b's ... then z's
    // Pre-fix tora declared concat with a fixed 1-arg
    // signature so multi-arg calls failed at the unified
    // arity check. Subset constraint kept: every additional
    // arg must be an Array<T> with the same element type
    // as the receiver — scalar args (the spec's "values
    // are added") would require the heterogeneous-element
    // substrate that isn't in tora yet.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "concat"
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let expected = (**elem).clone();
            // 0-arg form: shallow copy of receiver. Skip
            // arg-type validation entirely.
            if args.is_empty() {
                return Ok(Type::Array(Box::new(expected)));
            }
            // ES §23.1.3.2 — every arg is either an Array<T>
            // (spread into the result) or a single T value
            // (appended as one element). Mixed shapes are
            // valid: `xs.concat([4,5], 6, [7,8])`.
            let mut ok = true;
            for a in args {
                let a_ty = checker.type_of(ast, *a)?;
                let is_arr_t = a_ty == Type::Array(Box::new(expected.clone()));
                let is_scalar_t = a_ty == expected;
                if !is_arr_t && !is_scalar_t {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Ok(Type::Array(Box::new(expected)));
            }
        }
    }
    // V3-18 wedge — Array.copyWithin with 1 or 2 args per
    // JS spec §22.1.3.3:
    //   xs.copyWithin(target)            = (target, 0, len)
    //   xs.copyWithin(target, start)     = (target, start, len)
    //   xs.copyWithin(target, start, end)= (target, start, end)
    // Pre-fix tora declared the method with a fixed 3-arg
    // signature so `xs.copyWithin(0, 2)` failed at the
    // arity check. SSA lower already had the 3-arg code
    // path; this commit additionally fills the missing
    // start (= 0) / end (= len) defaults at the SSA layer.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "copyWithin"
        && args.len() >= 1
        && args.len() <= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            // S219 — accept Undefined for any arg per ES
            // §23.1.3.4: target/start go through
            // ToIntegerOrInfinity (undef → 0), end===undefined
            // takes the omitted default (len). ssa_lower
            // mirror short-circuits each undef slot to its
            // spec default before relative_to_len.
            // S335 — `xs.copyWithin(Any, Any, Any)` per ES
            // §23.1.3.4: each arg goes through
            // ToIntegerOrInfinity which accepts arbitrary-
            // typed input. Sister to S334.
            for (i, a) in args.iter().enumerate() {
                let a_ty = checker.type_of(ast, *a)?;
                if a_ty != Type::Number && a_ty != Type::Undefined && a_ty != Type::Any {
                    return Err(format!(
                        "Array.copyWithin arg {i} must be number, got {a_ty:?}"
                    ));
                }
            }
            return Ok(Type::Array(elem.clone()));
        }
    }
    // V3-18 m1.h.45 — String.padStart / padEnd with 1 arg
    // defaults the fill string to " " per JS spec §21.1.3.16.
    // Pre-fix tora declared the methods with 2 fixed params
    // so `s.padStart(3)` failed at the arity check.
    //
    // S201 — extend the same default-undefined rule to the
    // 0-arg call per ES §22.1.3.{16,17} step 1:
    // `intMaxLength = ToLength(undefined) = 0`, so the
    // step-2 short-circuit `intMaxLength <= S.length`
    // makes the no-arg form a no-op returning S unchanged.
    //
    // S223 — widen the same default-undefined rule to a
    // 1- or 2-arg call with an explicit `undefined` in the
    // maxLength slot. Routing the standard 2-arg shape
    // (`s.padStart(3, "*")`) through the same carve-out is
    // equivalent to the line-3413 Function sig but lets us
    // accept Undefined for arg 0.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "padStart" || m_name == "padEnd")
        && args.len() <= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            // S338 — `s.{padStart,padEnd}(Any [, fillStr])`
            // per ES §22.1.3.{16,17} step 1: ToLength
            // accepts arbitrary-typed input. Sister to
            // S332/S334. ssa_lower mirror routes Any
            // through anyv_to_number → coerce_to_i64.
            if let Some(arg0) = args.first() {
                let aty = checker.type_of(ast, *arg0)?;
                if !matches!(aty, Type::Number | Type::Undefined | Type::Any) {
                    return Err(format!("String.{m_name} arg 0 must be number, got {aty:?}"));
                }
            }
            if let Some(arg1) = args.get(1) {
                let aty = checker.type_of(ast, *arg1)?;
                // S236 — accept Undefined for the fillStr
                // slot per ES §22.1.3.{16,17} step 6.a: if
                // fillString is undefined, set it to " ".
                // ssa_lower_str's V3-18 m1.h.45 1-arg
                // fallthrough already supplies the " "
                // default, so we just need the type gate
                // to accept the typed-Undefined operand.
                if !matches!(aty, Type::String | Type::Undefined) {
                    return Err(format!("String.{m_name} arg 1 must be string, got {aty:?}"));
                }
            }
            return Ok(Type::String);
        }
    }
    // V3-18 m1.h.42 — Array<String|Substr>.join() with no
    // sep arg defaults to ","; matches JS spec §22.1.3.13.
    // Pre-fix tora declared join with 1 fixed param so
    // `xs.join()` failed at the arity check.
    //
    // S206 — extend the same rule to an explicit
    // `undefined` sep per spec §23.1.3.16 step 1: if
    // sep is undefined → sep = ",". The 1-arg-undefined
    // call shape was rejecting at the strict-arity
    // gate with "argument 0: expected String, got
    // Undefined".
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "join"
    {
        let undef_sep = args.len() == 1 && {
            let aty = checker.type_of(ast, args[0])?;
            matches!(aty, Type::Undefined)
        };
        if args.is_empty() || undef_sep {
            let src_ty = checker.type_of(ast, *src_id)?;
            if let Type::Array(elem) = &src_ty
                && matches!(
                    **elem,
                    Type::String | Type::Number | Type::Boolean | Type::Any
                )
            {
                return Ok(Type::String);
            }
        }
    }
    // V3-18 m1.h.36 — String.slice / substring with 0 or
    // 1 args. Per JS spec §21.1.3.21 / §21.1.3.23:
    //   s.slice()      = s.slice(0, s.length)
    //   s.slice(start) = s.slice(start, s.length)
    //   (same for substring; substring also clamps and
    //   swaps args, but the optional-arity shape is
    //   identical at the call site)
    //
    // S232 — accept Undefined for the start slot per ES
    // §22.1.3.20 step 3 (slice) / §22.1.3.22 step 4
    // (substring): ToIntegerOrInfinity(undef)=0. The
    // ssa_lower_str mirror substitutes ConstI64(0) and
    // the existing 0-1-arg fallthrough fills in the
    // end=length default. `substr` is excluded — its
    // 2nd arg is a length not an end index, and the
    // T-49 carve-out below handles its own arity-undef.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "slice" || m_name == "substring" || m_name == "substr")
        && args.len() < 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let allow_undef = m_name == "slice" || m_name == "substring";
            // S333 — `s.{slice,substring,substr}(Any)` per ES
            // §22.1.3.{20,22,23}: ToIntegerOrInfinity accepts
            // arbitrary-typed input. Widen the strict-Number
            // gate; ssa_lower mirror routes Any through
            // anyv_to_number → coerce_to_i64 → helper. Sister
            // to S332 (charCodeAt/charAt/... Any).
            for &aid in args {
                let aty = checker.type_of(ast, aid)?;
                if aty != Type::Number
                    && !(allow_undef && aty == Type::Undefined)
                    && aty != Type::Any
                {
                    return Err(format!("String.{m_name} arg must be number, got {aty:?}"));
                }
            }
            return Ok(Type::String);
        }
    }
    // S221 — `s.substring(start, end)` accepts Undefined for
    // either positional per ES §22.1.3.22 step 4/5:
    // ToIntegerOrInfinity(undef)=0 for start, end===undefined
    // takes the omitted default (len). ssa_lower mirror
    // short-circuits each undef slot to 0 / recv.length so
    // the str_substring helper's I64 ABI never sees an
    // Undefined operand.
    //
    // S232 — extend the same widen to `s.slice(start, end)`
    // 2-arg per ES §22.1.3.20 step 3/4: start undef → 0,
    // end undef → length. slice's negative-index handling
    // is orthogonal — the helper still receives concrete
    // I64s after the ssa_lower mirror substitutes.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "substring" || m_name == "slice" || m_name == "substr")
        && args.len() == 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            // S333 — 2-arg form widens Any. Same Any coerce
            // pattern A as the 1-arg path. substr extends the
            // method-table sig (Number, Number) here so the
            // (Any, Any) shape doesn't fall through to the
            // strict gate; ssa_lower mirror decodes via
            // anyv_to_number → coerce_to_i64.
            let allow_undef = m_name == "substring" || m_name == "slice";
            for &aid in args {
                let aty = checker.type_of(ast, aid)?;
                if aty != Type::Number
                    && !(allow_undef && aty == Type::Undefined)
                    && aty != Type::Any
                {
                    return Err(format!("String.{m_name} arg must be number, got {aty:?}"));
                }
            }
            return Ok(Type::String);
        }
    }
    // S241 — String.{slice,substring,substr,padStart,padEnd}
    // (a, b, ...trailing) trailing-arg ignore per ES
    // §22.1.3.{20,22,23,16,17}: spec reserves slots past the
    // 2 useful args (start/end / start/length / maxLen/fillStr)
    // but tora's helpers are 2-arg only. Trailing operand
    // type_of'd for side effects then dropped at lower-time
    // (ssa_lower break early past i=1). Same shape as S238
    // localeCompare.
    //
    // S284 — widen from `args.len() == 3` (single trailing)
    // to `args.len() >= 3` (any trailing count). ssa_lower
    // mirror swaps the loop break to lower_expr + continue
    // so step()-style side-effect exprs fire per ES eval-
    // then-discard semantics (S272 idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "slice" | "substring" | "substr" | "padStart" | "padEnd"
        )
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty0 = checker.type_of(ast, args[0])?;
            let arg0_ok = match m_name.as_str() {
                "slice" | "substring" => {
                    matches!(aty0, Type::Number | Type::Undefined)
                }
                "substr" => matches!(aty0, Type::Number),
                "padStart" | "padEnd" => {
                    matches!(aty0, Type::Number | Type::Undefined)
                }
                _ => false,
            };
            if !arg0_ok {
                return Err(format!(
                    "String.{m_name} arg 0 must be number, got {aty0:?}"
                ));
            }
            let aty1 = checker.type_of(ast, args[1])?;
            let arg1_ok = match m_name.as_str() {
                "slice" | "substring" => {
                    matches!(aty1, Type::Number | Type::Undefined)
                }
                "substr" => matches!(aty1, Type::Number),
                "padStart" | "padEnd" => {
                    matches!(aty1, Type::String | Type::Undefined)
                }
                _ => false,
            };
            if !arg1_ok {
                return Err(format!("String.{m_name} arg 1 type mismatch, got {aty1:?}"));
            }
            for &a in &args[2..] {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::String);
        }
    }
    // S211 — String.localeCompare(undefined) per ES
    // §22.1.3.10 step 4: thatStr = ToString(thatValue)
    // = "undefined". Pre-fix declared `(String) -> Number`
    // rejected the typed-Undefined arg with
    // "argument 0: expected String, got Undefined".
    // ssa_lower inline-substitutes the interned
    // "undefined" literal for the typed-undefined operand.
    //
    // S238 — extend the same carve-out to the 2-arg
    // (locales) and 3-arg (locales, options) shapes per ES
    // §22.1.3.10 trailing-arg ignore: the spec reserves
    // those slots for Intl-aware locale comparison but
    // tora's bytewise helper has no locale awareness, so
    // they're ignored. The ssa_lower_str loop trims any
    // arg beyond i=0 so the helper's (Str, Str) ABI never
    // sees the trailing operands.
    // S285 — widen S238 carve-out `(1..=3)` → `>= 1` so
    // 4+ arg trailing-widen shape typechecks. ssa_lower
    // mirror swaps the loop `break i > 0` to `let _ =
    // lower_expr(a); continue` so step()-style side-effect
    // exprs fire per ES eval-then-discard (S272 idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "localeCompare"
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty0 = checker.type_of(ast, args[0])?;
            let arg0_ok = matches!(aty0, Type::String | Type::Undefined);
            if arg0_ok {
                for &aid in &args[1..] {
                    let _ = checker.type_of(ast, aid)?;
                }
                return Ok(Type::Number);
            }
        }
    }
    // S292 — Array<T>.keys(...trailing) trailing-arg ignore
    // per ES §23.1.3.16. Spec sig is 0-arg returning an
    // ArrayIterator (tora: Type::ArrIter via
    // arr_iter_create_keys, fed only `recv_op`). Trailing
    // operands typecheck-and-drop here; ssa_lower mirrors
    // with lower-and-drop. values / entries stay narrow to
    // Array<Any> per the existing carve-out at 4540.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "keys"
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Array(_)) {
            for &aid in args.iter() {
                let _ = checker.type_of(ast, aid)?;
            }
            return Ok(Type::ArrIter);
        }
    }
    // S290 — primitive `.valueOf(...trailing)` trailing-arg
    // ignore per ES §21.1.3.27 / §20.4.3.4 / §22.1.3.34 /
    // §21.2.3.6 / §20.5.3.5. valueOf is 0-arg spec; tora's
    // SSA-emit folds it to an identity return (recv_op)
    // without inspecting args, so trailing operands type-
    // check-and-drop here + lower-and-drop in ssa_lower
    // (S272 idiom). Covers Number / Boolean / String /
    // BigInt / Symbol; Array.valueOf already handled by
    // the dedicated identity arm in ssa_lower.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "valueOf"
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        let ret_ty = match src_ty {
            Type::Number => Some(Type::Number),
            Type::Boolean => Some(Type::Boolean),
            Type::String => Some(Type::String),
            Type::BigInt => Some(Type::BigInt),
            Type::Symbol => Some(Type::Symbol),
            Type::Array(ref elem) => Some(Type::Array(elem.clone())),
            _ => None,
        };
        if let Some(rt) = ret_ty {
            for &aid in args.iter() {
                let _ = checker.type_of(ast, aid)?;
            }
            return Ok(rt);
        }
    }
    // S288 — Array<T>.{pop,shift}(...trailing) trailing-arg
    // ignore per ES §23.1.3.{20,24}. Spec sigs are 0-arg;
    // tora's runtime helpers (in-place len-- + tail/head
    // slot load) ignore any extras. Typecheck-and-drop
    // here; ssa_lower's try_arr_pop / try_arr_shift arms
    // widen the `args.is_empty()` gate + lower-and-drop
    // args[..] so step()-style exprs fire (S272 idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "pop" | "shift")
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let inner = (**elem).clone();
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(inner);
        }
    }
    // S287 — Array<T>.{reverse,toReversed,join,toString,
    // toLocaleString}(...trailing) trailing-arg ignore per
    // ES §23.1.3.{27,33,15,32,31}. reverse / toReversed /
    // toString / toLocaleString are 0-arg per spec; join is
    // 1-arg (sep). tora's runtime helpers all read at most
    // the documented slots, so trailing operands typecheck-
    // and-drop here, ssa_lower mirrors with lower-and-drop
    // in the dispatch arms (S272 idiom). join's sep slot
    // accepts String | Undefined per S206; toString /
    // toLocaleString gate on the join-compatible elem types
    // (Str/Num/Bool/Any per S126-4 dispatch).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "reverse" | "toReversed" | "join" | "toString" | "toLocaleString"
        )
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let inner = (**elem).clone();
            let useful = if m_name == "join" { 1 } else { 0 };
            if args.len() > useful {
                if useful == 1 {
                    let sep_ty = checker.type_of(ast, args[0])?;
                    if !matches!(sep_ty, Type::String | Type::Undefined) {
                        return Err(format!(
                            "Array.join arg 0 (sep) must be string, got {sep_ty:?}"
                        ));
                    }
                }
                for &aid in &args[useful..] {
                    let _ = checker.type_of(ast, aid)?;
                }
                return Ok(match m_name.as_str() {
                    "reverse" | "toReversed" => Type::Array(Box::new(inner)),
                    _ => Type::String,
                });
            }
        }
    }
    // S286 — String.{match,matchAll}(re, ...trailing) trailing-
    // arg ignore per ES §22.1.3.{11,13}. Spec reads only `re`;
    // tora's regex helper takes only (Str, RegExp) so trailing
    // operands typecheck-and-drop here, ssa_lower mirrors with
    // lower-and-drop in the RegExp-branch match/matchAll arms.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "match" | "matchAll")
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty0 = checker.type_of(ast, args[0])?;
            if matches!(aty0, Type::RegExp) {
                for &aid in &args[1..] {
                    let _ = checker.type_of(ast, aid)?;
                }
                return Ok(if m_name == "matchAll" {
                    Type::Array(Box::new(Type::Array(Box::new(Type::String))))
                } else {
                    Type::Array(Box::new(Type::String))
                });
            }
        }
    }
    // S246 — Array<T>.{copyWithin,fill}(a, b, c, ...trailing)
    // trailing-arg ignore per ES §23.1.3.{4,7}. Spec
    // reserves slots past the 3 useful args but tora's
    // inline copyWithin / fill loops are 3-arg only.
    // Trailing operand type_of'd for side effects then
    // dropped at lower-time; SSA-emit reads only
    // args[0..=2] so args[3..] are silently ignored.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "copyWithin" | "fill")
        && args.len() >= 4
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let inner = (**elem).clone();
            // arg 0 (target / value): type-checked per
            // method shape — fill arg 0 is element type,
            // copyWithin arg 0 is Number (target index).
            let aty0 = checker.type_of(ast, args[0])?;
            if m_name == "fill" {
                if aty0 != inner && !matches!(inner, Type::Any) {
                    return Err(format!(
                        "Array.fill arg 0 (value) must match elem type {:?}, got {aty0:?}",
                        inner
                    ));
                }
            } else if !matches!(aty0, Type::Number | Type::Undefined) {
                return Err(format!(
                    "Array.copyWithin arg 0 (target) must be number, got {aty0:?}"
                ));
            }
            let aty1 = checker.type_of(ast, args[1])?;
            if !matches!(aty1, Type::Number | Type::Undefined) {
                return Err(format!("Array.{m_name} arg 1 must be number, got {aty1:?}"));
            }
            let aty2 = checker.type_of(ast, args[2])?;
            if !matches!(aty2, Type::Number | Type::Undefined) {
                return Err(format!("Array.{m_name} arg 2 must be number, got {aty2:?}"));
            }
            // S310 — widen `== 4` to `>= 4` per ES §23.1.3.{4,7}
            // trailing-arg ignore. Spec reads target/value +
            // start + end only; trailing slots silent-drop.
            // ssa_lower's S298 skip(3) loop already drains
            // args[3..] for side-effects.
            for &a in args.iter().skip(3) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::Array(elem.clone()));
        }
    }
    // S245 — Array<T>.{reduce,reduceRight}(fn, init, ...trailing)
    // trailing-arg ignore per ES §22.1.3.{21,22}. Spec
    // reserves slots past the 2 useful args (callback +
    // initial value) but tora's inline reduce loop is
    // 2-arg only. Trailing operand type_of'd for side
    // effects then dropped at lower-time; SSA-emit reads
    // only args[0..=1] so args[2..] are silently ignored
    // without any SSA-side change. Same shape as S243/S244.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "reduce" | "reduceRight")
        && args.len() >= 3
    {
        // S276 — widen `== 3` to `>= 3` per ES §23.1.3.{22,23}
        // trailing-arg ignore. Spec reads cb + initialValue
        // only; trailing slots silent-drop. ssa_lower's
        // 22487 entry reads only args[0] (cb) + args[1]
        // (initial); args[2..] handled by S270 skip(2) loop.
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let inner = (**elem).clone();
            let aty0 = checker.type_of(ast, args[0])?;
            let fn_ok = matches!(aty0, Type::Function(..) | Type::Any);
            if !fn_ok {
                return Err(format!(
                    "Array.{m_name} arg 0 must be a callback function, got {aty0:?}"
                ));
            }
            let aty1 = checker.type_of(ast, args[1])?;
            if aty1 != inner && !matches!(inner, Type::Any) {
                return Err(format!(
                    "Array.{m_name} arg 1 (initial) must match elem type {:?}, got {aty1:?}",
                    inner
                ));
            }
            for &a in &args[2..] {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(inner);
        }
    }
    // S242 — Array<T>.{at,slice,join}(useful, ...trailing)
    // trailing-arg ignore per ES §23.1.3.{1,28,16}. Spec
    // reserves slots past the useful args but tora's
    // helpers are 1-/2-/1-arg only; trailing operand
    // type_of'd for side effects then dropped at lower-
    // time (ssa_lower's args[N] slot never read).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "at" | "slice" | "join")
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            // S299 — widen `args.len() == 2` → `>= 2` + typecheck-
            // and-drop args[1..] for any extra trailing operands per
            // ES §23.1.3.1 trailing-arg ignore (same family as S272/
            // S278/S293-S298). ssa_lower mirror widens at arm gate
            // from `args.len() <= 2` to no upper-cap + lower-and-drop
            // args[1..] so step()-style side-effect exprs fire per
            // ES eval-then-discard semantics.
            if m_name == "at" && args.len() >= 2 {
                let aty0 = checker.type_of(ast, args[0])?;
                if !matches!(aty0, Type::Number | Type::Undefined) {
                    return Err(format!("Array.at arg 0 must be number, got {aty0:?}"));
                }
                for &a in args.iter().skip(1) {
                    let _ = checker.type_of(ast, a)?;
                }
                return Ok((**elem).clone());
            }
            // S299 — widen `== 3` → `>= 3` + drop args[2..] per ES
            // §23.1.3.28 trailing-arg ignore.
            if m_name == "slice" && args.len() >= 3 {
                let aty0 = checker.type_of(ast, args[0])?;
                if !matches!(aty0, Type::Number | Type::Undefined) {
                    return Err(format!("Array.slice arg 0 must be number, got {aty0:?}"));
                }
                let aty1 = checker.type_of(ast, args[1])?;
                if !matches!(aty1, Type::Number | Type::Undefined) {
                    return Err(format!("Array.slice arg 1 must be number, got {aty1:?}"));
                }
                for &a in args.iter().skip(2) {
                    let _ = checker.type_of(ast, a)?;
                }
                return Ok(Type::Array(elem.clone()));
            }
            // S299 — widen `== 2` → `>= 2` + drop args[1..] per ES
            // §23.1.3.16 trailing-arg ignore. ssa_lower's join arm
            // already loop-drops args[1..] via the S287 useful=1
            // skip; widening the typecheck gate completes the pair.
            if m_name == "join"
                && args.len() >= 2
                && matches!(
                    **elem,
                    Type::String | Type::Number | Type::Boolean | Type::Any
                )
            {
                let aty0 = checker.type_of(ast, args[0])?;
                if !matches!(aty0, Type::String | Type::Undefined) {
                    return Err(format!("Array.join arg 0 must be string, got {aty0:?}"));
                }
                for &a in args.iter().skip(1) {
                    let _ = checker.type_of(ast, a)?;
                }
                return Ok(Type::String);
            }
        }
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
    // S278 — widen `args.len() == 3` → `>= 3` + typecheck-
    // and-drop args[2..] for any extra trailing operands per
    // ES trailing-arg ignore (same family as S270/S272/S275/
    // S276/S277). ssa_lower mirror widens the Array path
    // gate to `>= 1` + lower-and-drop args[2..]; the String
    // path swaps `break` to `let _ = lower_expr(a); continue`
    // so step()-style side-effect exprs fire per ES eval-
    // then-discard semantics.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "indexOf" | "lastIndexOf" | "includes" | "startsWith" | "endsWith"
        )
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let needle_ty = checker.type_of(ast, args[0])?;
            if !matches!(needle_ty, Type::String | Type::Undefined) {
                return Err(format!(
                    "String.{m_name} arg 0 must be string, got {needle_ty:?}"
                ));
            }
            let from_ty = checker.type_of(ast, args[1])?;
            if !matches!(from_ty, Type::Number | Type::Undefined) {
                return Err(format!(
                    "String.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                ));
            }
            for &a in args.iter().skip(2) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(
                if matches!(m_name.as_str(), "includes" | "startsWith" | "endsWith") {
                    Type::Boolean
                } else {
                    Type::Number
                },
            );
        }
        if let Type::Array(elem) = &src_ty
            && matches!(m_name.as_str(), "indexOf" | "lastIndexOf" | "includes")
        {
            let needle_ty = checker.type_of(ast, args[0])?;
            if needle_ty != **elem && !matches!(**elem, Type::Any) {
                return Err(format!(
                    "Array.{m_name} arg 0 must match elem type {:?}, got {needle_ty:?}",
                    **elem
                ));
            }
            let from_ty = checker.type_of(ast, args[1])?;
            if !matches!(from_ty, Type::Number | Type::Undefined) {
                return Err(format!(
                    "Array.{m_name} arg 1 (fromIndex) must be number, got {from_ty:?}"
                ));
            }
            for &a in args.iter().skip(2) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(if m_name == "includes" {
                Type::Boolean
            } else {
                Type::Number
            });
        }
    }
    // S255 — Object.keys / Object.getOwnPropertyNames /
    // Reflect.ownKeys (obj, ...trailing) trailing-arg ignore
    // per ES §20.1.2.{17,22} / §28.1.11. Spec reads only
    // args[0]; tora silent-drops trailing per generic
    // trailing-arg-ignore policy. SSA-emit mirror widens
    // the `args.len() == 1` gate to `>= 1` (ssa_lower.rs:
    // ~18745). Narrow to this shared 3-method SSA-emit
    // dispatch — other Object/Reflect methods need
    // per-method SSA-emit widening (L3b).
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ((ns == "Object" && (m_name == "keys" || m_name == "getOwnPropertyNames"))
            || (ns == "Reflect" && m_name == "ownKeys"))
        && args.len() >= 2
    {
        let _ = checker.type_of(ast, args[0])?;
        for &arg in args.iter().skip(1) {
            let _ = checker.type_of(ast, arg)?;
        }
        return Ok(Type::Array(Box::new(Type::String)));
    }
    // S256 — Object.{entries,freeze,isFrozen}(obj, ...trailing)
    // trailing-arg ignore per ES §20.1.2.{5,12,15}. Spec
    // reads only args[0]; tora silent-drops trailing per
    // generic trailing-arg-ignore policy. SSA-emit mirror
    // widens each `args.len() == 1` gate to `>= 1`.
    // S258 extends to `values` — same shape, returns
    // Array<Any> (the per-method 1-arg sig was missing
    // entirely; now added above).
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Object"
        && matches!(
            m_name.as_str(),
            "entries" | "freeze" | "isFrozen" | "values"
        )
        && args.len() >= 2
    {
        let arg0_ty = checker.type_of(ast, args[0])?;
        for &arg in args.iter().skip(1) {
            let _ = checker.type_of(ast, arg)?;
        }
        return Ok(match m_name.as_str() {
            "entries" => Type::Array(Box::new(Type::Array(Box::new(Type::Any)))),
            "freeze" => arg0_ty,
            "isFrozen" => Type::Boolean,
            "values" => Type::Array(Box::new(Type::Any)),
            _ => unreachable!(),
        });
    }
    // S257 — Object.{hasOwn,is} / Reflect.{has,get} (obj,
    // key|b, ...trailing) trailing-arg ignore per ES
    // §20.1.2.{4,9} / §28.1.{6,9}. Spec reads only
    // args[0..2]; tora silent-drops trailing. SSA-emit
    // mirror widens each `args.len() == 2` gate to `>= 2`
    // (ssa_lower.rs ~18649/18710/19802). Narrow to these
    // 4 SSA-emit dispatches — Reflect.get's 3rd `receiver`
    // arg is spec-meaningful only for Proxy targets (tora
    // has no Proxy substrate), silent-drop is spec-correct
    // for the non-Proxy case tora supports.
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ((ns == "Object" && matches!(m_name.as_str(), "hasOwn" | "is"))
            || (ns == "Reflect" && matches!(m_name.as_str(), "has" | "get")))
        && args.len() >= 3
    {
        let _ = checker.type_of(ast, args[0])?;
        let _ = checker.type_of(ast, args[1])?;
        for &arg in args.iter().skip(2) {
            let _ = checker.type_of(ast, arg)?;
        }
        return Ok(match m_name.as_str() {
            "hasOwn" | "is" | "has" => Type::Boolean,
            "get" => Type::Any,
            _ => unreachable!(),
        });
    }
    // S262 — Boolean/Symbol/String × {toString,toLocaleString}
    // trailing-arg ignore per ES §20.3.3.{2,3} / §20.4.3.3
    // / §22.1.3.{27,28}. Each method's `Vec::new()` sig
    // rejected 1+ arg calls; SSA-emit already silent-drops
    // user args (ssa_lower.rs ~16565 Symbol / ~16579 Bool
    // both push only `vec![recv_op]`; ~16593 String returns
    // recv_op identity). Narrow widen: matches m_name +
    // 3 receiver types + args.len() >= 1, returns String.
    // (Number.toLocaleString already handled by S260 above
    // with the 2-arg sig; Number.toString radix handled by
    // S244 in the wedge.)
    if let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "toString" | "toLocaleString")
        && !args.is_empty()
    {
        let recv_ty = checker.type_of(ast, *recv_id)?;
        if matches!(recv_ty, Type::Boolean | Type::Symbol | Type::String) {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::String);
        }
    }
    // S304 — Struct instance Object.prototype methods
    // trailing-arg ignore per ES §20.1.3.{2,3,4,5,7}. Useful
    // arity: toString/valueOf:0, hasOwnProperty/propertyIs
    // Enumerable/isPrototypeOf:1. tora's struct-instance arms
    // (4619-4632) declared fixed sigs that rejected trailing
    // operands at the generic arity check. ssa_lower's struct
    // dispatch never reads args[useful..] (toString → "[object
    // Object]" const; hasOwnProperty → layout scan keyed on
    // args[0] String literal), so trailing typecheck-and-drop
    // here suffices — no SSA mirror needed. (BigInt.toLocale
    // String trailing also surfaced during probe but its
    // 0-arg form is itself unsupported by ssa_lower — kept
    // in L3b until the substrate lands.)
    if let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
    {
        let recv_ty = checker.type_of(ast, *recv_id)?;
        let useful = match (&recv_ty, m_name.as_str()) {
            (Type::Struct(_), "toString") | (Type::Struct(_), "valueOf") => Some((0, false)),
            (Type::Struct(_), "hasOwnProperty")
            | (Type::Struct(_), "propertyIsEnumerable")
            | (Type::Struct(_), "isPrototypeOf") => Some((1, true)),
            _ => None,
        };
        if let Some((useful, _is_bool)) = useful
            && args.len() > useful
        {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            let ret = match m_name.as_str() {
                "toString" => Type::String,
                "toLocaleString" => Type::String,
                "valueOf" => recv_ty.clone(),
                "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => Type::Boolean,
                _ => unreachable!(),
            };
            return Ok(ret);
        }
    }
    // S261 — Date instance 0-arg getter / format method
    // family (`d.getTime() / d.getFullYear() / d.toISOString() /
    // d.toLocaleString() / ...`) trailing-arg ignore per ES
    // §21.4.4.*. Spec methods either take 0 args or have
    // optional `key`/`locales`/`options` args that tora's
    // subset silently drops; the SSA-emit default branch
    // (ssa_lower.rs ~21601 `else { vec![recv_op] }`) only
    // forwards recv_op for these 0-arg methods, so trailing
    // user args naturally drop. Narrow widen: matches the
    // 0-arg method list, Type::Date receiver, args.len() >= 1.
    if let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "getTime"
                | "valueOf"
                | "toISOString"
                | "toJSON"
                | "getFullYear"
                | "getUTCFullYear"
                | "getMonth"
                | "getUTCMonth"
                | "getDate"
                | "getUTCDate"
                | "getHours"
                | "getUTCHours"
                | "getMinutes"
                | "getUTCMinutes"
                | "getSeconds"
                | "getUTCSeconds"
                | "getMilliseconds"
                | "getUTCMilliseconds"
                | "getDay"
                | "getUTCDay"
                | "getTimezoneOffset"
                | "getYear"
                | "toGMTString"
                | "toUTCString"
                | "toDateString"
                | "toLocaleString"
                | "toLocaleDateString"
                | "toLocaleTimeString"
        )
        && !args.is_empty()
    {
        let recv_ty = checker.type_of(ast, *recv_id)?;
        if matches!(recv_ty, Type::Date) {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(
                if matches!(
                    m_name.as_str(),
                    "toISOString"
                        | "toJSON"
                        | "toGMTString"
                        | "toUTCString"
                        | "toDateString"
                        | "toLocaleString"
                        | "toLocaleDateString"
                        | "toLocaleTimeString"
                ) {
                    Type::String
                } else {
                    Type::Number
                },
            );
        }
    }
    // S260 — `n.toLocaleString(locales?, options?, ...trailing)`
    // trailing-arg ignore per ES §21.1.3.4. Spec reads
    // locales + options + ignores rest; tora ignores all
    // formatting args already (en-US-only subset; ssa_lower
    // pass_args=false at line ~16707). The fixed-arity
    // `vec![Any, Any]` sig above rejects 3+ arg calls —
    // widen here. Type-erased silent-drop matches the
    // SSA-emit reality (no arg flows to runtime helper).
    if let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "toLocaleString"
        && args.len() >= 3
    {
        let recv_ty = checker.type_of(ast, *recv_id)?;
        if matches!(recv_ty, Type::Number) {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::String);
        }
    }
    // S259 — Symbol.{for,keyFor}(key|s, ...trailing)
    // trailing-arg ignore per ES §19.4.{2,3}. Spec reads
    // only args[0]; tora silent-drops trailing. SSA-emit
    // mirror widens `args.len() == 1` gate to `>= 1`
    // (ssa_lower.rs ~18877).
    if let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*ns_id)
        && ns == "Symbol"
        && matches!(m_name.as_str(), "for" | "keyFor")
        && args.len() >= 2
    {
        let _ = checker.type_of(ast, args[0])?;
        for &arg in args.iter().skip(1) {
            let _ = checker.type_of(ast, arg)?;
        }
        return Ok(match m_name.as_str() {
            "for" => Type::Symbol,
            "keyFor" => Type::Nullable(Box::new(Type::String)),
            _ => unreachable!(),
        });
    }
    // S248 — Set.add / Map.set (value, ...trailing) /
    // (key, value, ...trailing) trailing-arg ignore per
    // ES §24.2.3.1 (Set.prototype.add) / §23.1.3.9
    // (Map.prototype.set). Spec adds the useful args then
    // returns the receiver for chained idiom; tora widens
    // the dispatch to accept trailing operands and drops
    // them at lower-time (ssa-lower debug-assert widens
    // `== N` → `>= N`; receiver-chain rc_inc unchanged).
    // Same narrow-shape as S243-S247.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "add" | "set")
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Set) && m_name == "add" && args.len() >= 2 {
            let _ = checker.type_of(ast, args[0])?;
            for &arg in &args[1..] {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Set);
        }
        if matches!(src_ty, Type::Map) && m_name == "set" && args.len() >= 3 {
            let _ = checker.type_of(ast, args[0])?;
            let _ = checker.type_of(ast, args[1])?;
            for &arg in &args[2..] {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Map);
        }
    }
    // S269 — Object.{create,setPrototypeOf,defineProperties}
    // trailing-arg ignore per ES §20.1.2.{1,5,21}. tora's
    // fixed sigs (`vec![Type::Any]` for create / `vec![
    // Type::Any, Type::Any]` for the other two) rejected
    // the next arg; SSA-emit's intercept for all three
    // already eval-and-drops args[1..] (`for a in args`
    // / `for a in args.iter().skip(1)`), so the lower path
    // is safe — S269 widens checktime to accept the matching
    // floor and beyond.
    //
    // S317 — extend the same widen to `defineProperty(obj,
    // key, desc, ...trailing)` per ES §20.1.2.6. fixed
    // sig `vec![Type::Any, Type::String, Type::Any]` (3
    // args) rejected the 4th; paired ssa_lower change
    // widens `args.len() == 3` to `>= 3` + lowers-and-
    // drops args[3..] after `emit_define_one` for spec
    // left-to-right side-effect order.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "create" | "setPrototypeOf" | "defineProperties" | "defineProperty"
        )
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("Object")) {
            let floor: usize = match m_name.as_str() {
                "create" => 2,
                "setPrototypeOf" | "defineProperties" => 3,
                "defineProperty" => 4,
                _ => unreachable!(),
            };
            if args.len() >= floor {
                for &arg in args.iter() {
                    let _ = checker.type_of(ast, arg)?;
                }
                return Ok(match m_name.as_str() {
                    "defineProperties" | "defineProperty" => Type::Void,
                    _ => Type::Any,
                });
            }
        }
    }
    // S268 — Date instance setter trailing-arg ignore per
    // ES §21.4.4.{20-26}: each per-field setter accepts up
    // to N Number args (year/month/date/hours/etc); trailing
    // args beyond that silent-drop per ES trailing-arg
    // ignore. tora's fixed-N sigs rejected the next arg;
    // SSA-emit's `for i in 0..target_arity` loop already
    // takes only args[0..arity] so trailing operands fall
    // off the slice (release-build); pair with a skip(arity)
    // eval-and-drop loop in the matching widen below to
    // preserve side effects and replace the
    // `debug_assert!(args.len() <= arity)`.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "setFullYear"
                | "setMonth"
                | "setDate"
                | "setHours"
                | "setMinutes"
                | "setSeconds"
                | "setMilliseconds"
                | "setTime"
                | "setYear"
        )
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Date) {
            let max_arity: usize = match m_name.as_str() {
                "setFullYear" | "setMinutes" => 3,
                "setMonth" | "setSeconds" => 2,
                "setDate" | "setMilliseconds" | "setTime" | "setYear" => 1,
                "setHours" => 4,
                _ => unreachable!(),
            };
            if args.len() > max_arity {
                for &arg in args.iter() {
                    let _ = checker.type_of(ast, arg)?;
                }
                return Ok(Type::Number);
            }
        }
    }
    // S267 — Array.isArray(value, ...trailing) per ES
    // §23.1.2.2. The spec uses only step 1's `value`;
    // trailing args silent-drop. tora's check.rs sig
    // `vec![Type::Any] -> Boolean` rejected 1+ trailing
    // with "expected 1 argument(s), got N"; ssa_lower's
    // intercept already routes through args[0] only, so
    // a skip(1) eval-and-drop loop preserves side effects
    // for any trailing expression.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "isArray"
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("Array")) {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Boolean);
        }
    }
    // S266 — RegExp.{test,exec,toString}(s?, ...trailing)
    // per ES §22.2.6.{2,7,16}. Each method's fixed sig
    // (`vec![Type::String]` / `Vec::new()`) rejected 1+
    // trailing args. SSA-emit uses only args[0] (test/exec)
    // or no args (toString); the matching widen relaxes
    // the debug-assert + eval-and-drops args[1..].
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "test" | "exec" | "toString")
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::RegExp)
            && matches!(m_name.as_str(), "test" | "exec")
            && args.len() >= 2
        {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(match m_name.as_str() {
                "test" => Type::Boolean,
                "exec" => Type::Array(Box::new(Type::String)),
                _ => unreachable!(),
            });
        }
        if matches!(src_ty, Type::RegExp) && m_name == "toString" && !args.is_empty() {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::String);
        }
    }
    // S265 — Object.{getPrototypeOf,isExtensible,isSealed,
    // preventExtensions,seal}(obj, ...trailing) per ES
    // §20.1.2.{12,13,14,16,18,20}. Each method's fixed sig
    // (`vec![Type::Any]`) rejected 1+ trailing args; ssa_lower
    // for the 4 anyv_* helpers already drops args[1..] via
    // `for a in args.iter().skip(1) { let _ = checker.lower_expr(*a); }`;
    // getPrototypeOf gets the same treatment in the matching
    // widen below. Returns the original Any (proto/cell) or
    // Boolean per the underlying sig.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "getPrototypeOf" | "isExtensible" | "isSealed" | "preventExtensions" | "seal"
        )
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("Object")) {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(match m_name.as_str() {
                "isExtensible" | "isSealed" => Type::Boolean,
                _ => Type::Any,
            });
        }
    }
    // S264 — Set/Map instance method trailing-arg ignore
    // per ES §24.2.3.{4,5,7} (Set.{delete,clear,has}) +
    // §23.1.3.{3,4,6,7} (Map.{delete,clear,get,has}).
    // Each method's fixed sig (vec![Any] / Vec::new())
    // rejected 1+ trailing args; ssa_lower's strict
    // `debug_assert_eq!(args.len(), 1)` / `args.is_empty()`
    // becomes a `>= N` floor in the matching widen below.
    // (Set.add / Map.set already covered by S248.)
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "has" | "delete" | "get" | "clear")
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        // (Set|Map).{has,delete} accept >= 2 (key + trail).
        if matches!(src_ty, Type::Set | Type::Map)
            && matches!(m_name.as_str(), "has" | "delete")
            && args.len() >= 2
        {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Boolean);
        }
        // Map.get accepts >= 2 (key + trail).
        if matches!(src_ty, Type::Map) && m_name == "get" && args.len() >= 2 {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Nullable(Box::new(Type::Any)));
        }
        // (Set|Map).clear accept >= 1 (trail only).
        if matches!(src_ty, Type::Set | Type::Map) && m_name == "clear" && !args.is_empty() {
            for &arg in args.iter() {
                let _ = checker.type_of(ast, arg)?;
            }
            return Ok(Type::Void);
        }
    }
    // S301 — WeakMap.{set,get,has,delete} + WeakSet.{add,has,
    // delete} trailing-arg ignore per ES §24.{3,4}.3.*. Useful
    // arity is set:2 / others:1; fixed-sig declarations rejected
    // trailing args with "expected N argument(s), got M".
    // typecheck-and-drop args[useful..] mirror in ssa_lower (the
    // WeakMap/WeakSet lower dispatch loop also caps useful when
    // building full_args). Same family as S264 Set/Map block.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        let useful = match (&src_ty, m_name.as_str()) {
            (Type::WeakMap, "set") => Some(2),
            (Type::WeakMap, "get") | (Type::WeakMap, "has") | (Type::WeakMap, "delete") => Some(1),
            (Type::WeakSet, "add") | (Type::WeakSet, "has") | (Type::WeakSet, "delete") => Some(1),
            _ => None,
        };
        if let Some(useful) = useful
            && args.len() > useful
        {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            let ret = match (&src_ty, m_name.as_str()) {
                (Type::WeakMap, "set") => Type::Void,
                (Type::WeakMap, "get") => Type::Nullable(Box::new(Type::Any)),
                (Type::WeakSet, "add") => Type::Void,
                _ => Type::Boolean,
            };
            return Ok(ret);
        }
    }
    // S318 — Set ES2025 setops trailing-arg ignore per ES
    // §24.2.5.{4-10}: spec sig is 1-arg (other SetLike);
    // trailing args silent-drop. Pre-S318 the fixed
    // (Type::Set, "isSubsetOf"|...) method-table sig
    // `vec![Type::Set]` rejected `args.len() >= 2` with
    // "expected 1 argument(s), got M". typecheck-drop
    // args[1..] mirror in ssa_lower (replace
    // `debug_assert_eq!(args.len(), 1)` carve-out with
    // lower-and-drop loop after args[0] lower).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "isSubsetOf"
                | "isSupersetOf"
                | "isDisjointFrom"
                | "union"
                | "intersection"
                | "difference"
                | "symmetricDifference"
        )
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Set) {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(match m_name.as_str() {
                "isSubsetOf" | "isSupersetOf" | "isDisjointFrom" => Type::Boolean,
                _ => Type::Set,
            });
        }
    }
    // S315 — Object.getOwnPropertyDescriptor(obj, key, ...trailing)
    // per ES §20.1.2.10: spec sig is 2-arg; trailing args MUST
    // be silently ignored. Pre-S315 the fixed (Type::Object(
    // "Object"), "getOwnPropertyDescriptor") method-table sig
    // rejected `args.len() >= 3` with "expected 2 argument(s),
    // got M". typecheck-drop trailing + ssa_lower mirror at
    // 18024 widens `== 2` → `>= 2` and adds skip(2) loop.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "getOwnPropertyDescriptor"
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("Object")) {
            // Useful arg typecheck (obj: Any, key: String).
            let _ = checker.type_of(ast, args[0])?;
            let aty1 = checker.type_of(ast, args[1])?;
            if !matches!(aty1, Type::String) {
                return Err(format!(
                    "Object.getOwnPropertyDescriptor arg 1 (key) must be string, got {aty1:?}"
                ));
            }
            for &a in args.iter().skip(2) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::Any);
        }
    }
    // S314 — BigInt.{asIntN,asUintN}(bits, value, ...trailing)
    // per ES §21.2.2.{1,2}: spec sig is 2-arg; trailing args
    // MUST be silently ignored. Pre-S314 the fixed (Type::
    // Object("BigInt"), m) method-table sig rejected
    // `args.len() >= 3` with "expected 2 argument(s), got M".
    // typecheck-drop trailing + ssa_lower mirror widens gate
    // + lower-and-drop trailing.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "asIntN" | "asUintN")
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("BigInt")) {
            // Useful arg typecheck (bits: Number, value: BigInt).
            let aty0 = checker.type_of(ast, args[0])?;
            if !matches!(aty0, Type::Number) {
                return Err(format!(
                    "BigInt.{m_name} arg 0 (bits) must be number, got {aty0:?}"
                ));
            }
            let aty1 = checker.type_of(ast, args[1])?;
            if !matches!(aty1, Type::BigInt) {
                return Err(format!(
                    "BigInt.{m_name} arg 1 (value) must be bigint, got {aty1:?}"
                ));
            }
            for &a in args.iter().skip(2) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::BigInt);
        }
    }
    // S309 — Object.fromEntries(entries, ...trailing) per ES
    // §20.1.2.7: spec sig is 1-arg (entries iterable);
    // trailing args are silently ignored. Pre-S309 the
    // fixed (Type::Object("Object"), "fromEntries") method-
    // table sig rejected `args.len() >= 2` with "expected
    // 1 argument(s), got M". typecheck-drop args[1..] +
    // ssa_lower mirror (LetDecl fast-path widens
    // is_fromentries_call gate + lower-and-drop trailing).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "fromEntries"
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::Object("Object")) {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::Any);
        }
    }
    // S210 — String.search() / search(undefined) per ES
    // §22.1.3.20: RegExpCreate(undefined, undefined)
    // yields an empty regex which matches at index 0.
    // Pre-fix tora declared `(String) -> Number`, so
    // 0-arg failed at arity and 1-arg-undefined failed
    // with "argument 0: expected String, got Undefined".
    // ssa_lower short-circuits to ConstI64(0).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "search"
        && args.len() < 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            if let Some(&aid) = args.first() {
                let aty = checker.type_of(ast, aid)?;
                if matches!(aty, Type::Undefined) {
                    return Ok(Type::Number);
                }
            } else {
                return Ok(Type::Number);
            }
        }
    }
    // S325 — WeakRef.deref(...trailing) trailing-arg ignore
    // per ES §26.1.3.2. spec is 0-arg; tora's static-table sig
    // `(WeakRef, "deref") -> Function([], Nullable<Any>)` at
    // ~3661 rejected 1+ arg calls at strict arity. Carve-out
    // typecheck-and-drops args[..]; ssa_lower mirror peeks the
    // receiver via expr_types and lower-and-drops trailing.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "deref"
        && !args.is_empty()
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::WeakRef) {
            for &a in args.iter() {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::Nullable(Box::new(Type::Any)));
        }
    }
    // S324 — String.search(needle, ...trailing) trailing-arg
    // ignore per ES §22.1.3.20. spec reads only needle; tora's
    // declared `(String) -> Number` sig at ~4194 rejected 2+ arg
    // calls at strict arity. Mirror the S240-family widen (1-
    // useful methods): typecheck-and-drop args[1..] and let the
    // existing (String) -> Number arm pick up the result type.
    // ssa_lower_str dispatch loop adds `"search"` to the S240
    // 1-useful trailing-drop list so step()-style trailing args
    // fire and the (Str, Str) helper ABI never sees them.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "search"
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let needle_ty = checker.type_of(ast, args[0])?;
            if !matches!(needle_ty, Type::String | Type::Undefined) {
                return Err(format!(
                    "String.search arg 0 must be string, got {needle_ty:?}"
                ));
            }
            for &a in args.iter().skip(1) {
                let _ = checker.type_of(ast, a)?;
            }
            return Ok(Type::Number);
        }
    }
    // S209 — String.repeat(undefined) per ES §22.1.3.17
    // step 1: ToIntegerOrInfinity(undefined) = 0 → return
    // the empty string. Declared `vec![Type::Number] ->
    // String` rejected the typed-Undefined arg with
    // "argument 0: expected Number, got Undefined".
    // ssa_lower_str pushes ConstI64(0) for the missing
    // count and routes through str_repeat as usual.
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "repeat"
        && args.len() == 1
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let aty = checker.type_of(ast, args[0])?;
            if matches!(aty, Type::Undefined) {
                return Ok(Type::String);
            }
        }
    }
    // S207 — String.replace / replaceAll with fewer-than-2
    // args per ES §22.1.3.18 / §22.1.3.19 step 4 + step 6a:
    //   searchString = ToString(searchValue ?? undefined)
    //                = "undefined"
    //   replaceValue = ToString(replaceValue ?? undefined)
    //                = "undefined"
    // Declared `(Any, Any) -> String` would reject 0/1-arg
    // shapes at the strict-arity gate; widen here and let
    // ssa_lower_str push the missing "undefined" interns
    // so the helper sees a valid Str needle / replacement.
    // Bun-aligned: 0-arg → identity unless haystack contains
    // "undefined"; 1-arg → first match of needle replaced
    // by the literal "undefined".
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && (m_name == "replace" || m_name == "replaceAll")
        && args.len() < 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            // Populate expr_types for any present arg so
            // ssa_lower_str can detect a typed-undefined
            // operand (via expr_types.get) and substitute
            // the interned "undefined" literal instead of
            // emitting a non-Str operand the helper would
            // deref as a Str pointer.
            for &aid in args {
                checker.type_of(ast, aid)?;
            }
            return Ok(Type::String);
        }
    }
    // S339 — `xs.with(Any idx, val)` per ES §23.1.3.39 step 2:
    // ToIntegerOrInfinity accepts arbitrary-typed input. The
    // method-table sig `(Number, T) -> Array<T>` at the Array
    // dispatch site (~4422) strict-rejected `o: any` operands
    // at typecheck ('argument 0: expected Number, got Any').
    // Widen here so the ssa_lower mirror routes Any through
    // anyv_to_number → coerce_to_i64 → arr_with helper.
    // Sister to S331/S332/S333/S334/S335. Covers both basic
    // 2-arg case and trailing-arg ≥3 case (S283 sibling
    // below stays strict-Number for non-Any args[0]).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "with"
        && args.len() >= 2
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let aty0 = checker.type_of(ast, args[0])?;
            if matches!(aty0, Type::Any) {
                let inner = (**elem).clone();
                let aty1 = checker.type_of(ast, args[1])?;
                if aty1 != inner && !matches!(inner, Type::Any) {
                    return Err(format!(
                        "Array.with arg 1 (value) must match elem type {:?}, got {aty1:?}",
                        inner
                    ));
                }
                for &aid in &args[2..] {
                    let _ = checker.type_of(ast, aid)?;
                }
                return Ok(Type::Array(Box::new(inner)));
            }
        }
    }
    // S283 — Array.prototype.with(index, value, ...trailing)
    // trailing-arg ignore per ES §23.1.3.39. Spec reads only
    // index + value; tora's arr_with intrinsic is 2-arg only.
    // Widen check.rs to accept args.len() >= 3; ssa_lower
    // mirror widens the `args.len() == 2` gate to `>= 2`
    // and lowers args[2..] for side effects (S272 idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && m_name == "with"
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if let Type::Array(elem) = &src_ty {
            let inner = (**elem).clone();
            let aty0 = checker.type_of(ast, args[0])?;
            if !matches!(aty0, Type::Number) {
                return Err(format!(
                    "Array.with arg 0 (index) must be number, got {aty0:?}"
                ));
            }
            let aty1 = checker.type_of(ast, args[1])?;
            if aty1 != inner && !matches!(inner, Type::Any) {
                return Err(format!(
                    "Array.with arg 1 (value) must match elem type {:?}, got {aty1:?}",
                    inner
                ));
            }
            for &aid in &args[2..] {
                let _ = checker.type_of(ast, aid)?;
            }
            return Ok(Type::Array(Box::new(inner)));
        }
    }
    // S282 — String.{replace,replaceAll,split}(useful, useful,
    // ...trailing) trailing-arg ignore per ES §22.1.3.{18,19,
    // 21}. Spec reads only 2 args (search/replace or
    // separator/limit); tora's helpers are 2-arg only.
    // Widen check.rs to accept args.len() >= 3; ssa_lower
    // mirror lowers args[2..] for side effects then drops
    // the values (S272 idiom).
    if let Expr::Member {
        obj: src_id,
        name: m_name,
    } = ast.get_expr(*callee)
        && matches!(m_name.as_str(), "replace" | "replaceAll" | "split")
        && args.len() >= 3
    {
        let src_ty = checker.type_of(ast, *src_id)?;
        if matches!(src_ty, Type::String) {
            let _ = checker.type_of(ast, args[0])?;
            let _ = checker.type_of(ast, args[1])?;
            for &aid in &args[2..] {
                let _ = checker.type_of(ast, aid)?;
            }
            return Ok(match m_name.as_str() {
                "replace" | "replaceAll" => Type::String,
                "split" => Type::Array(Box::new(Type::String)),
                _ => unreachable!(),
            });
        }
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
    if args.len() >= params.len() + 1
        && let Expr::Member { name: m_name, .. } = ast.get_expr(*callee)
        && matches!(
            m_name.as_str(),
            "map"
                | "filter"
                | "every"
                | "some"
                | "forEach"
                | "find"
                | "findIndex"
                | "findLast"
                | "findLastIndex"
                | "flatMap"
        )
    {
        // S270 — widen the thisArg drop from `== params+1`
        // to `>= params+1` so any trailing args past thisArg
        // are also silent-dropped per ES §23.1.3.X trailing-
        // arg ignore (xs.map(cb, thisArg, ...trailing) is
        // spec-legal — spec uses only cb + thisArg). Type-
        // check every dropped arg so expr's internal errors
        // still surface; SSA-emit reads only args[0] (cb).
        for &arg in &effective_args[params.len()..] {
            let _ = checker.type_of(ast, arg)?;
        }
        effective_args.truncate(params.len());
    }
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
