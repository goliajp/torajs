//! What the builtin namespaces answer — the fixed half of
//! [`super::desugar_generators_field_ann`].
//!
//! Split out when the Index arm took the parent to its 500-line
//! limit. The divide is what the two halves ask: the parent asks
//! "what IS this initializer" and grows one arm per expression shape
//! that turned out to be pinned to `number`; this file answers "what
//! does this NAME answer", off a table the spec fixes, and grows only
//! when a builtin's signature is settled enough to list.
//!
//! Same rule on both sides, and it is the load-bearing one: an absent
//! entry DECLINES, which returns the field to the `number` fallback.
//! A wrong answer here pins the field exactly as badly, so a
//! signature goes in when it is settled, not when it is guessed.

use super::Ast;
use super::desugar_generators_walkers::LiftCtx;

/// What a call to a global constructor answers, by spec: §22.1.1
/// `String(x)`, §20.3.1 `Boolean(x)`, §20.4.1 `Symbol(x)`. Only
/// reached for a name with no local binding, so a user's own `String`
/// shadows this exactly as it shadows everything else.
///
/// `Number(x)` is deliberately absent. Its answer is not in doubt —
/// but `number` alone does not say i64 or f64, and `Number("7")`
/// produces an f64 the container width analysis does not see coming:
/// annotating the field took the shape from one loud failure to
/// another ("f64 value into i64 struct field — container width
/// analysis missed this write"). It goes back in when that write is
/// seen, not before.
pub(super) fn global_ctor_ann(name: &str) -> Option<String> {
    match name {
        "String" => Some("string".into()),
        "Boolean" => Some("boolean".into()),
        "Symbol" => Some("symbol".into()),
        _ => None,
    }
}

/// What an `Object` static answers, mirroring the checker's own
/// signatures (`check_type_of_member_object_meta` /
/// `check_type_of_member_reflect` / `check_type_of_member_namespace`)
/// — the sniff wanted the receiver's annotation and `Object` is a
/// namespace, not a value it can type, so every one of these declined
/// and the field took the `number` fallback. `let d = Object.
/// getOwnPropertyDescriptor(o, "k")` in a generator printed `0`, and
/// `d.configurable` after it said "no member `.configurable` on type
/// Number" (the t262 forbidden-ext b2 family, 45 cases).
///
/// Only the signatures whose checker answer is settled are listed; an
/// absent name declines to the fallback exactly as before, because a
/// wrong answer here pins the field just as badly (the rule
/// `promise_static_ann` already follows for the combinators).
pub(super) fn object_static_ann(name: &str) -> Option<String> {
    Some(
        match name {
            "getOwnPropertyDescriptor"
            | "getOwnPropertyDescriptors"
            | "getPrototypeOf"
            | "setPrototypeOf"
            | "create"
            | "assign"
            | "fromEntries"
            | "freeze"
            | "defineProperty"
            | "defineProperties" => "any",
            "keys" | "getOwnPropertyNames" => "string[]",
            "values" => "any[]",
            "is" | "isFrozen" | "hasOwn" => "boolean",
            _ => return None,
        }
        .into(),
    )
}

/// What a `Promise` static answers, or None to leave the call alone.
///
/// `resolve(x)` is the one that matters here and the one §27.2.4.7
/// makes precise: handing it a promise passes that promise's value
/// through, so the annotation is the argument's own; handing it a
/// plain value makes that value the promise's. `reject` and the
/// combinators are left to decline — each has its own value rule
/// (`allSettled` answers an array of outcome objects, not of values),
/// and a wrong one here pins the field just as badly as the fallback
/// it replaces.
pub(super) fn promise_static_ann(
    ast: &Ast,
    name: &str,
    args: &[super::ExprId],
    ctx: &LiftCtx,
) -> Option<String> {
    if name != "resolve" {
        return None;
    }
    let Some(arg) = args.first() else {
        // `Promise.resolve()` fulfils with `undefined`.
        return Some("Promise<any>".into());
    };
    let inner = super::desugar_generators_field_ann::field_ann(ast, *arg, ctx)?;
    if inner.starts_with("Promise<") {
        return Some(inner);
    }
    Some(format!("Promise<{inner}>"))
}
