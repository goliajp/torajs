//! Explicit-Undefined / Any 1-arg spec-corner widen arms
//! extracted from [`crate::check_type_of_call::check`]'s
//! top-level cascade (chunk 223 — sixteenth sub-batch of
//! check_type_of_call.rs per-shape decomposition).
//!
//! All four share the pattern "1-arg `Member` call whose
//! standard `Type::Function` sig rejects an explicit
//! `Undefined` (or `Any`) argument, but the JS spec routes
//! that through `ToNumber` / `ToString` / `ToUint16` to a
//! well-defined constant or coerced result." Widening here
//! lets ssa_lower fold the call appropriately at lower-time.
//!
//! - **S230** — `Date.parse(undefined)` (ES §21.4.3.2):
//!   `ToString(undefined) = "undefined"` → invalid date →
//!   `NaN`. ssa_lower folds to `ConstF64(NaN)`. Returns
//!   `Number`.
//!
//! - **S227** — `Math.<unary>(undefined)` for the 30+ NaN-
//!   propagating Math unary methods (sqrt / abs / floor /
//!   ceil / log / exp / sign / round / trunc / sin / cos /
//!   tan / asin / acos / atan / log2 / log10 / cbrt / sinh
//!   / cosh / tanh / asinh / acosh / atanh / expm1 / log1p
//!   / clz32 / fround / f16round). `ToNumber(undefined) =
//!   NaN`; ssa_lower folds to `ConstF64` without lowering
//!   the arg. `Math.clz32(undefined)` is intentionally
//!   included even though it returns `32`, since the result
//!   is still `Number`. Returns `Number`.
//!
//! - **S231 / S329** — `String.fromCharCode(undefined | Any)`
//!   (ES §22.1.2.1): each arg goes through `ToUint16`;
//!   `ToUint16(undefined) = 0` yields `"\0"`. The `Any` path
//!   routes through `anyv_to_number → coerce_to_i64 →
//!   helper`. Returns `String`.
//!
//! - **S340** — `String.fromCodePoint(Any)` (ES §22.1.2.2
//!   step 2): `ToNumber` accepts arbitrary-typed input;
//!   `RangeError` throw shape enforced by runtime helper
//!   `str_from_code_point` (pending throw +
//!   emit_throw_check). Returns `String`. Explicit Undefined
//!   diverges (bun throws RangeError — alignment is L3b).
//!
//! Returns `Some(Ok(_))` when one of the 4 patterns matches;
//! `Some(Err(_))` on arg typecheck failure; `None` when none
//! of them match (cascade falls through to the regular
//! static-sig dispatch path).

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

pub(crate) fn try_match(
    checker: &mut Checker,
    ast: &Ast,
    callee: &ExprId,
    args: &Vec<ExprId>,
) -> Option<Result<Type, String>> {
    // S230 — Date.parse(undefined) → NaN.
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "Date"
        && m == "parse"
        && args.len() == 1
    {
        let arg_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(arg_ty, Type::Undefined) {
            return Some(Ok(Type::Number));
        }
    }
    // S227 — Math.<unary>(undefined) NaN-propagating set.
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "Math"
        && args.len() == 1
    {
        let arg_ty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if matches!(arg_ty, Type::Undefined)
            && matches!(
                m.as_str(),
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
            )
        {
            return Some(Ok(Type::Number));
        }
    }
    // S231 / S329 — `String.{fromCharCode,fromCodePoint}(x)` 1-arg.
    // Both spec steps (§22.1.2.1 `ToUint16`, §22.1.2.2 `ToNumber`)
    // COERCE their operand, so every shape is admitted; `Number`
    // alone falls through to the strict namespace table so the
    // typed-tier fast path never boxes. Rotation 463 merged what
    // were two adjacent per-shape blocks (`Undefined | Any` for
    // fromCharCode, `Any` for fromCodePoint) — the difference
    // between them was never a spec fact, only which shape had been
    // asked for first, and `String.fromCharCode("0")` is "\0".
    if let Expr::Member { obj, name: m } = ast.get_expr(*callee)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && ns == "String"
        && matches!(m.as_str(), "fromCharCode" | "fromCodePoint")
        && args.len() == 1
    {
        let aty = match checker.type_of(ast, args[0]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if !matches!(aty, Type::Number) {
            return Some(Ok(Type::String));
        }
    }
    None
}
