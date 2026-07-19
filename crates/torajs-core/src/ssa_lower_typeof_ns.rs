//! `typeof <ns>.<member>` classification — layers 3-4 of
//! [`crate::ssa_lower_typeof`]'s cascade, carved out when that file hit
//! the 500-line limit.
//!
//! Layer 3: an `Object.prototype` method name on ANY receiver is a
//! function (`typeof o.hasOwnProperty`). Layer 4: a member of a known
//! namespace (`Math` / `JSON` / `Symbol` / ...) classifies by shape —
//! well-known Symbols are symbols, Math / Number constants and
//! `.length` are numbers, `.prototype` is an object, `.name` a string,
//! everything else a function.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_member_typeof(ctx: &LowerCtx<'_>, expr: ExprId) -> Option<&'static str> {
    // Layer 3: the Object-prototype names whose value read still
    // cannot be trusted, answered by name alone.
    //
    // This list used to carry seven names, which made `typeof` lie
    // about any object that shadows one of them
    // (`{ toString: undefined }` answered "function"). Four of them
    // now resolve correctly at runtime and have been dropped:
    // hasOwnProperty, propertyIsEnumerable, valueOf and
    // isPrototypeOf all read back as real callables on every
    // receiver shape tested, shadowed or not.
    //
    // The three that stay are the ones a runtime answer would get
    // wrong, both tracked in plan-state L3b:
    //   - constructor is not a method at all and has no value-read
    //     support, so runtime says "undefined" everywhere.
    //   - toString / toLocaleString read back on plain objects now,
    //     but not on closures — and there the call itself throws, so
    //     declaring them readable would hand out a value that cannot
    //     be called. Keeping the shortcut preserves bun's answer for
    //     the closure receiver at the cost of still lying about a
    //     shadowed toString.
    if let Expr::Member {
        name: member_name, ..
    } = ctx.ast.get_expr(expr)
        && matches!(
            member_name.as_str(),
            "constructor" | "toString" | "toLocaleString"
        )
    {
        return Some("function");
    }
    // Layer 4: namespace member dispatch.
    let Expr::Member {
        obj,
        name: member_name,
    } = ctx.ast.get_expr(expr)
    else {
        return None;
    };
    let Expr::Ident(ns) = ctx.ast.get_expr(*obj) else {
        return None;
    };
    if !is_known_ns(ns) {
        return None;
    }
    Some(classify_ns_member(ns, member_name))
}

fn is_known_ns(ns: &str) -> bool {
    matches!(
        ns,
        "Math"
            | "JSON"
            | "Reflect"
            | "globalThis"
            | "console"
            | "Object"
            | "Array"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "Date"
            | "RegExp"
            | "Error"
            | "BigInt"
            | "Promise"
            | "Map"
            | "Set"
    )
}

fn classify_ns_member(ns: &str, member_name: &str) -> &'static str {
    let is_symbol_well_known = ns == "Symbol"
        && matches!(
            member_name,
            "iterator"
                | "asyncIterator"
                | "toPrimitive"
                | "toStringTag"
                | "hasInstance"
                | "isConcatSpreadable"
                | "match"
                | "replace"
                | "search"
                | "split"
                | "species"
                | "unscopables"
        );
    let is_math_const = ns == "Math"
        && matches!(
            member_name,
            "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "SQRT2" | "SQRT1_2"
        );
    let is_number_const = ns == "Number"
        && matches!(
            member_name,
            "MAX_VALUE"
                | "MIN_VALUE"
                | "MAX_SAFE_INTEGER"
                | "MIN_SAFE_INTEGER"
                | "EPSILON"
                | "POSITIVE_INFINITY"
                | "NEGATIVE_INFINITY"
                | "NaN"
        );
    if is_symbol_well_known {
        "symbol"
    } else if is_math_const || is_number_const || member_name == "length" {
        "number"
    } else if member_name == "prototype" {
        "object"
    } else if member_name == "name" {
        "string"
    } else {
        "function"
    }
}
