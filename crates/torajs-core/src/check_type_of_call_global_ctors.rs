//! Global-bareword ctor / coercion call shapes extracted
//! from [`crate::check_type_of_call::check`]'s top-level
//! `if let Expr::Ident(n) = ast.get_expr(*callee)` cascade
//! (chunk 208 — second sub-batch of check_type_of_call.rs
//! per-shape decomposition; mirrors chunk 207).
//!
//! Covers the 4 bare-Ident global call shapes that route
//! BEFORE the regular ident-callee dispatch table:
//! - `fetch(url)` → Promise<Response> (T-21 v0.6.0)
//! - `Number(x)` / `String(x)` / `Boolean(x)` callable
//!   coercion (NOT `new` — wrapper-object form deferred);
//!   spec §21.1.1 / §22.1.1 / §20.3.1. Per-input type
//!   dispatch covers Number / Boolean / Null / Undefined /
//!   String / Any / Array / Struct / ClassRef (String also
//!   takes Symbol and Function).
//!   Trailing args silently dropped per generic policy.
//! - `BigInt(value)` callable ctor (V3-03); arg type
//!   dispatched (bigint clone / string from_str / number
//!   from_number). Trailing args dropped.
//! - `Symbol(desc?)` ctor (T-13.a); optional String desc;
//!   missing → NULL pointer at runtime, prints "Symbol()".
//!   Trailing args dropped.
//!
//! Returns `Some(Ok(_))` on match success, `Some(Err(_))` on
//! arg shape mismatch within a matched ctor, `None` when
//! none of the 4 names match.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    /* T-21 (v0.6.0) — `fetch(url)` returns Promise<Response>.
     * Response has `.text(): Promise<string>` and a `status:
     * number` property; both are SSA-level operations on
     * the heap-alloc'd Response struct populated by
     * `__torajs_fetch_sync`. POST / headers / method /
     * body land in a fetch options follow-up. */
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == "fetch"
    {
        if args.len() != 1 {
            return Some(Err(format!(
                "fetch(url) expects 1 string arg, got {}",
                args.len()
            )));
        }
        let url_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(url_ty, Type::String) {
            return Some(Err(format!(
                "fetch(url) — url must be string, got {url_ty:?}"
            )));
        }
        return Some(Ok(Type::Promise(Box::new(Type::Object("Response")))));
    }
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && (n == "Number" || n == "String" || n == "Boolean")
    {
        return try_primitive_coercion_ctor(checker, ast, n, args);
    }
    /* V3-03 — `BigInt(value)` callable ctor. One required
     * arg, type-dispatched by ssa_lower:
     *   bigint  → clone
     *   string  → from_str (auto-radix from prefix)
     *   number  → from_number (RangeError on non-integer
     *             / non-finite)
     * Type::Any is deferred (Any-tagged dispatch lands
     * with the test262 push). */
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == "BigInt"
    {
        if args.is_empty() {
            return Some(Err("BigInt(value) expects exactly 1 arg, got 0".to_string()));
        }
        let arg_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(arg_ty, Type::BigInt | Type::String | Type::Number) {
            return Some(Err(format!(
                "BigInt(value) — value must be bigint / string / number, got {arg_ty:?}"
            )));
        }
        // S308 — typecheck-and-drop trailing args[1..] per ES
        // §21.2.1 trailing-arg ignore. Spec coerces only args[0];
        // ssa_lower mirror at ~16022 reads only args[0] so
        // args[1..] dropped at lower-time without further
        // change. Mirror requires lower-and-drop in SSA.
        for &a in args.iter().skip(1) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::BigInt));
    }
    // T-13.a (v0.4.0) — `Symbol(desc?)` constructor call.
    // Returns Type::Symbol. Optional desc; missing (or `undefined`)
    // desc = NULL pointer at runtime, prints `Symbol()`.
    //
    // §20.4.1.1 step 3 is `descString = ? ToString(description)`, so
    // EVERY value shape is legal here — `Symbol(1)` is the description
    // "1". The shape that does fail (a Symbol description) fails
    // because ToString throws, which is a catchable runtime TypeError,
    // not a compile-time reject: `assert.throws(TypeError, () =>
    // Symbol(sym))` is the test262 spelling and it needs the program
    // to compile. So no static gate — only the operand's own typing
    // errors propagate; the lowerer runs the ToString.
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == "Symbol"
    {
        if !args.is_empty()
            && let Err(e) = checker.type_of(ast, args[0])
        {
            return Some(Err(e));
        }
        // S308 — typecheck-and-drop trailing args[1..] per ES
        // §20.4.1 trailing-arg ignore (Symbol(desc, ...trailing)
        // — only desc is read; trailing dropped).
        for &a in args.iter().skip(1) {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Symbol));
    }
    // RFC 20260716 刀 4 — `Object(v)` callable coercion (ES
    // §20.1.1.1 + ToObject §7.1.18). Any arg type is legal:
    // primitives mint a fresh wrapper, heap objects identity,
    // null/undef fresh `{}`. Static result type is Any — the
    // per-input choice can't be tightened without a per-arg
    // switch on the runtime side (Type::Any is what the
    // Number/String/BooleanWrapper checker returns anyway).
    if let Expr::Ident(n) = ast.get_expr(*callee)
        && n == "Object"
    {
        for &a in args.iter() {
            if let Err(e) = checker.type_of(ast, a) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Type::Any));
    }
    None
}

/// V3-18 m1.h.8 — `Number(x)` / `String(x)` / `Boolean(x)` callable
/// coercion (NOT `new` — that's the wrapper-object form, deferred).
/// Spec §21.1.1 Number(value), §22.1.1 String(value), §20.3.1
/// Boolean(value): unconditionally coerce to the named primitive type.
/// ssa_lower's per-input dispatch covers Number+Bool+Null (and String
/// for the String() case); other arg types panic — String/Object →
/// Number ToString-then-parse path lands with the m1.h.9 wedge.
///
/// S251 — Number/String/Boolean(value, ...trailing) per ES §21.1.1 /
/// §22.1.1 / §20.3.1 trailing-arg ignore. Spec coerces only args[0];
/// tora silent-drops trailing per generic trailing-arg-ignore policy.
/// SSA-emit reads args[0] (or empty), so args[1..] dropped at
/// lower-time without further change.
fn try_primitive_coercion_ctor(
    checker: &mut Checker,
    ast: &Ast,
    n: &str,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    let result_ty = match n {
        "Number" => Type::Number,
        "String" => Type::String,
        "Boolean" => Type::Boolean,
        _ => unreachable!(),
    };
    for &arg in args.iter().skip(1) {
        if let Err(e) = checker.type_of(ast, arg) {
            return Some(Err(e));
        }
    }
    if let Some(a) = args.first() {
        let arg_ty = match checker.type_of(ast, *a) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let ok = match n {
            "Boolean" => true,
            // P1.5 — Number(undefined) === NaN per spec §7.1.4.
            // String(undefined) === "undefined" per §7.1.17.
            // S172 — Number(Array<T>) routes through ToPrimitive("string") =
            // arr.join(",") then ToNumber(String) (bun parity: Number([]) === 0,
            // Number([1]) === 1, Number([1,2]) === NaN).
            "Number" => matches!(
                arg_ty,
                Type::Number
                    | Type::Boolean
                    | Type::Null
                    | Type::Undefined
                    | Type::String
                    | Type::Any
                    | Type::Array(_)
                    // §7.1.4 step 8 — an object answers whatever
                    // OrdinaryToPrimitive(number) does: its own
                    // `valueOf` when it has one, NaN otherwise. The
                    // String arm beside this one has admitted both
                    // shapes for as long as it has run them.
                    | Type::Struct(_)
                    | Type::ClassRef(_)
                    // §21.1.1.1 step 3 — the explicit Number() call
                    // is the one legal BigInt→Number conversion
                    // (𝔽(ℝ(value))); implicit ToNumber keeps
                    // throwing.
                    | Type::BigInt
            ),
            // S137 — `String(arr)` routes to arr_join (ES §22.1.3.30 same path
            // as `arr.toString`); `String(struct)` is the generic Object
            // `[object Object]` per §20.1.4.4. Generic dynobj branch lands when
            // the dynobj-toString substrate ships.
            // rotation 141 — Symbol admitted: §22.1.1 step 1.a, the explicit
            // String() call is the one legal Symbol stringify position
            // (SymbolDescriptiveString).
            // RFC 20260719-fn-tostring-source B5 — Function admitted:
            // ToString(fn) = its toString() = the type-erased source
            // (fn_source_str kernel; the template-substitution String() wrap
            // rides this).
            "String" => matches!(
                arg_ty,
                Type::Number
                    | Type::Boolean
                    | Type::Null
                    | Type::Undefined
                    | Type::String
                    | Type::Any
                    | Type::Array(_)
                    | Type::Struct(_)
                    | Type::Symbol
                    | Type::Function(..)
                    // A class instance is an ordinary object here:
                    // §7.1.17 runs OrdinaryToPrimitive, which finds
                    // the class's own `toString` when it declares one
                    // and Object.prototype's otherwise. The any lane
                    // already answered both; only the typed spelling
                    // was turned away.
                    | Type::ClassRef(_)
            ),
            _ => false,
        };
        if !ok {
            return Some(Err(format!(
                "{n}({arg_ty:?}) coercion not yet supported (V3-18 m1.h.9 follow-up)"
            )));
        }
    }
    Some(Ok(result_ty))
}
