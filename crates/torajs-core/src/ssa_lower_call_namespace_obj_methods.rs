//! Namespace-object method dispatch on constructor identifiers
//! (`Number.<method>` / `String.<method>` / `Math.<method>` / etc).
//! Pulled out of [`crate::ssa_lower::lower_expr_inner`] `Expr::Call`
//! dispatch as chunk-38 of the `Expr::Call` god-arm decomp (chunks
//! 1-37 = ... + BigInt() callable ctor).
//!
//! V3-18 m2.b — Object.prototype subset on constructor-namespace
//! objects:
//!   - `hasOwnProperty(key)` — compile-time lookup; ConstBool(true)
//!     if `key` is a literal string matching a known own property of
//!     the namespace, else false. Non-literal key falls back to false
//!     (subset limitation; runtime per-namespace lookup table is a
//!     follow-up). args[0] side-effects + drop preserved.
//!   - `propertyIsEnumerable(key)` — always ConstBool(false). Built-
//!     in own properties are non-enumerable per spec. args[0] side-
//!     effects + drop preserved.
//!   - `isPrototypeOf(key)` — always ConstBool(false). args[0] side-
//!     effects + drop preserved.
//!   - `toString()` — interned literal
//!     `"function NS() { [native code] }"`.
//!
//! "Close enough" for the test262 cases probing the call-doesn't-
//! throw behaviour on constructor objects.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not a Member-
//! Ident, or namespace not in the allowlist, or method not in the
//! four handled names).

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

const NS_ALLOWLIST: &[&str] = &[
    "Number", "String", "Boolean", "Symbol", "BigInt", "Object", "Array", "Math", "JSON",
    "Reflect", "RegExp", "Date", "Error", "Promise", "Map", "Set", "console", "Function",
];

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: recv_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let recv_id = *recv_id;
    let method = m_name.clone();
    let Expr::Ident(ns) = ctx.ast.get_expr(recv_id) else {
        return None;
    };
    let ns = ns.clone();
    if !NS_ALLOWLIST.contains(&ns.as_str()) {
        return None;
    }
    match method.as_str() {
        "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" => {
            // Compile-time lookup: if args[0] is a string literal,
            // check whether the name is a known own property of the
            // namespace and emit ConstBool(true/false). Non-literal
            // arg falls back to false (subset limitation; runtime
            // per-namespace lookup table is a follow-up).
            //
            // Methods/properties that JS spec marks as own on each
            // constructor object; propertyIsEnumerable is
            // conservatively false for built-in own props (spec —
            // built-ins are non-enumerable), so split on m_name.
            let result = if let Some(arg_eid) = args.first()
                && let Expr::String(key) = ctx.ast.get_expr(*arg_eid)
            {
                let is_own = ns_has_own_property(&ns, key);
                method == "hasOwnProperty" && is_own
            } else {
                false
            };
            if !args.is_empty() {
                // The compile-time fold only inspects the AST; an Ident
                // arg is a borrow (keeps its stake), owned temps
                // release here (old consume+drop stole the stake).
                let arg_val = ctx.lower_expr(args[0]);
                ctx.release_owned_temp(args[0], &arg_val);
            }
            Some(Operand::ConstBool(result))
        }
        "toString" => {
            let s = format!("function {ns}() {{ [native code] }}");
            let v = ctx.intern_string_literal(&s);
            Some(Operand::Value(v))
        }
        _ => None,
    }
}

fn ns_has_own_property(ns: &str, key: &str) -> bool {
    match ns {
        "Number" => matches!(
            key,
            "NaN"
                | "POSITIVE_INFINITY"
                | "NEGATIVE_INFINITY"
                | "EPSILON"
                | "MAX_SAFE_INTEGER"
                | "MIN_SAFE_INTEGER"
                | "MAX_VALUE"
                | "MIN_VALUE"
                | "parseInt"
                | "parseFloat"
                | "isInteger"
                | "isNaN"
                | "isFinite"
                | "isSafeInteger"
                | "prototype"
                | "length"
                | "name"
        ),
        "String" => matches!(
            key,
            "fromCharCode" | "fromCodePoint" | "raw" | "prototype" | "length" | "name"
        ),
        "Boolean" => matches!(key, "prototype" | "length" | "name"),
        "Symbol" => matches!(
            key,
            "iterator"
                | "asyncIterator"
                | "toPrimitive"
                | "toStringTag"
                | "hasInstance"
                | "isConcatSpreadable"
                | "match"
                | "matchAll"
                | "replace"
                | "search"
                | "split"
                | "species"
                | "unscopables"
                | "for"
                | "keyFor"
                | "prototype"
                | "length"
                | "name"
        ),
        "BigInt" => matches!(key, "asIntN" | "asUintN" | "prototype" | "length" | "name"),
        "Object" => matches!(
            key,
            "keys"
                | "values"
                | "entries"
                | "assign"
                | "freeze"
                | "isFrozen"
                | "create"
                | "fromEntries"
                | "is"
                | "getOwnPropertyNames"
                | "getOwnPropertySymbols"
                | "getPrototypeOf"
                | "setPrototypeOf"
                | "defineProperty"
                | "defineProperties"
                | "getOwnPropertyDescriptor"
                | "preventExtensions"
                | "isExtensible"
                | "seal"
                | "isSealed"
                | "prototype"
                | "length"
                | "name"
        ),
        "Array" => matches!(
            key,
            "isArray" | "from" | "of" | "prototype" | "length" | "name"
        ),
        "Math" => matches!(
            key,
            "PI" | "E"
                | "LN2"
                | "LN10"
                | "LOG2E"
                | "LOG10E"
                | "SQRT2"
                | "SQRT1_2"
                | "abs"
                | "ceil"
                | "floor"
                | "round"
                | "trunc"
                | "sign"
                | "sqrt"
                | "cbrt"
                | "exp"
                | "log"
                | "log2"
                | "log10"
                | "log1p"
                | "expm1"
                | "pow"
                | "min"
                | "max"
                | "hypot"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "atan2"
                | "sinh"
                | "cosh"
                | "tanh"
                | "asinh"
                | "acosh"
                | "atanh"
                | "random"
                | "imul"
                | "clz32"
                | "fround"
                | "f16round"
        ),
        "JSON" => matches!(key, "stringify" | "parse" | "rawJSON" | "isRawJSON"),
        "Reflect" => matches!(
            key,
            "apply"
                | "construct"
                | "defineProperty"
                | "deleteProperty"
                | "get"
                | "getOwnPropertyDescriptor"
                | "getPrototypeOf"
                | "has"
                | "isExtensible"
                | "ownKeys"
                | "preventExtensions"
                | "set"
                | "setPrototypeOf"
        ),
        "Function" | "RegExp" | "Date" | "Error" | "Promise" | "Map" | "Set" => {
            matches!(key, "prototype" | "length" | "name")
        }
        _ => false,
    }
}
