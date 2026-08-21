//! `typeof <ns>.<member>` classification — layers 3-4 of
//! [`crate::ssa_lower_typeof`]'s cascade, carved out when that file hit
//! the 500-line limit.
//!
//! Layer 3: an `Object.prototype` method name on ANY receiver is a
//! function (`typeof o.hasOwnProperty`). Layer 4: a member of a known
//! namespace (`Math` / `JSON` / `Symbol` / ...) classifies by shape —
//! well-known Symbols are symbols, Math / Number constants and
//! `.length` are numbers, `.prototype` is an object, `.name` a string.
//! A name matching no shape is a function only if the namespace
//! actually has it; otherwise there is no static answer and the value
//! itself gets to speak.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_member_typeof(ctx: &LowerCtx<'_>, expr: ExprId) -> Option<&'static str> {
    // Layer 3: Object-prototype method names answered by name alone.
    //
    // All seven used to take this path on every receiver, which made
    // `typeof` lie about an object that shadows one of them
    // (`{ toString: undefined }` answered "function", and so did
    // `{ toString: 42 }`).
    //
    // Six of them now read back as real callables at runtime, so an
    // Any receiver is better served by the runtime answer and skips
    // the shortcut (toString / toLocaleString joined rotation 154 —
    // the closure-receiver value read the old note called
    // unsupported reads back fine, and the static shortcut made
    // `typeof { toString: undefined }.toString` answer "function"
    // where bun answers "undefined" and `{ toString: 42 }` "number").
    // The condition is deliberately narrow: only a statically-Any
    // receiver. A typed receiver has no runtime member-read path at
    // all — a struct instance lowers `.name` as a field access, so
    // skipping the shortcut there turns
    // `typeof inst.hasOwnProperty` into a hard "struct has no field
    // hasOwnProperty" lowering error (conformance
    // m2d-001-class-instance-prototype).
    //
    // `constructor` stays on every receiver: it has no value-read
    // support at all (plan-state L3b), so the runtime read would
    // answer undefined where "function" is right.
    if let Expr::Member {
        obj,
        name: member_name,
    } = ctx.ast.get_expr(expr)
    {
        let runtime_answers = matches!(
            member_name.as_str(),
            "hasOwnProperty"
                | "propertyIsEnumerable"
                | "isPrototypeOf"
                | "valueOf"
                | "toString"
                | "toLocaleString"
        ) && matches!(ctx.expr_types.get(obj), Some(crate::check::Type::Any));
        if !runtime_answers
            && matches!(
                member_name.as_str(),
                "constructor"
                    | "hasOwnProperty"
                    | "propertyIsEnumerable"
                    | "isPrototypeOf"
                    | "valueOf"
                    | "toString"
                    | "toLocaleString"
            )
        {
            return Some("function");
        }
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
    if let Some(s) = classify_ns_member(ns, member_name) {
        return Some(s);
    }
    // No shape matched, so the old catch-all answered "function" for
    // every remaining name — including ones the namespace does not
    // have. `typeof Math.nope` is "undefined", and if someone planted
    // `Math.prop = 7` it is "number"; neither is knowable here. When
    // the checker says the name is outside the modeled surface, fall
    // through and let the value answer for itself (the read rides the
    // any-member lane onto the singleton's own-entry dict). A modeled
    // name keeps the static "function".
    // Keyed off the namespace IDENT, not the receiver's recorded
    // type: `ctx.expr_types` has no entry for most of these receivers
    // (`Math` is one), so reading the type here answers None and the
    // gate silently never fires.
    if let Some(tag) = crate::check_type_of_member_global_miss::ecma_global_tag(ns)
        && crate::check_type_of_member_global_miss::member_unmodeled(tag, member_name)
    {
        return None;
    }
    Some("function")
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
            | "Iterator"
    )
}

/// The shape-based answers. `None` = no shape matched, which the
/// caller resolves against the modeled surface rather than assuming
/// "function".
fn classify_ns_member(ns: &str, member_name: &str) -> Option<&'static str> {
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
                | "matchAll"
                | "dispose"
                | "asyncDispose"
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
    Some(if is_symbol_well_known {
        "symbol"
    } else if is_math_const || is_number_const || member_name == "length" {
        "number"
    } else if member_name == "prototype" {
        "object"
    } else if member_name == "name" {
        "string"
    } else {
        return None;
    })
}
