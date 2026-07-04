//! Date instance-method arms extracted from the
//! [`crate::check_type_of_member::check`] top-level
//! `match (&obj_ty, name) { ... }` (chunk 191 — first
//! sub-batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 185-190 try_lower shape).
//!
//! Pure type-table lookup — every Date instance method
//! returns a fixed `Type::Function(args, ret)` literal
//! with no `Checker` / `Ast` state, so the helper is a
//! self-contained `(name: &str) -> Option<...>` table.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `name` is
//! not a Date instance method. Caller falls through to
//! the next type-family arm on `None`.

use crate::check::Type;

pub(crate) fn try_match(name: &str) -> Option<Result<Type, String>> {
    let ty = match name {
        // v0.2 #2 Phase 2.0a — Date instance methods.
        "getTime" | "valueOf" => Type::Function(Vec::new(), Box::new(Type::Number)),
        "toISOString" => Type::Function(Vec::new(), Box::new(Type::String)),
        // ES §21.4.4.37 — `d.toJSON(key?)` returns the
        // canonical JSON serialization, defined as
        // `this.toISOString()` for any finite Date. The
        // optional `key` argument is ignored per spec
        // (only meaningful to Object.toJSON callers). MVP
        // collapses to toISOString without the non-finite
        // null short-circuit; non-finite Dates are an
        // independent substrate (the underlying ms field
        // already rejects non-finite ToNumber). The
        // `key` arg is silently dropped here — the
        // ssa_lower-side arm doesn't pass it through.
        "toJSON" => Type::Function(Vec::new(), Box::new(Type::String)),
        // v0.2 #2 Phase 2.0b — UTC getters. Local-time
        // siblings (getFullYear etc.) collapse to UTC
        // until timezone awareness ships in Phase 2.0c.
        "getFullYear" | "getUTCFullYear" | "getMonth" | "getUTCMonth" | "getDate"
        | "getUTCDate" | "getHours" | "getUTCHours" | "getMinutes" | "getUTCMinutes"
        | "getSeconds" | "getUTCSeconds" | "getMilliseconds" | "getUTCMilliseconds" | "getDay"
        | "getUTCDay" | "getTimezoneOffset" => Type::Function(Vec::new(), Box::new(Type::Number)),
        // T-30 — Date setters + annexB methods. setTime
        // takes ms and returns the new ms. setYear takes
        // a year (annexB §B.2.4.2 — 0-99 → +1900) and
        // returns the new ms. getYear (annexB §B.2.4.1)
        // returns year - 1900. toGMTString (annexB §B.2.4.3)
        // is an alias for toUTCString format.
        "setTime" => Type::Function(vec![Type::Number], Box::new(Type::Number)),
        "setYear" => Type::Function(vec![Type::Number], Box::new(Type::Number)),
        // Per-field setters per ES §21.4.4.20-26. Each
        // takes 1-N Number args (trailing ones optional);
        // we expose them as taking N required Numbers
        // and let ssa_lower sentinel-pad missing trailing
        // args. Returns the new `d.ms` (Number) per spec.
        "setFullYear" => Type::Function(
            vec![Type::Number, Type::Number, Type::Number],
            Box::new(Type::Number),
        ),
        "setMonth" => Type::Function(vec![Type::Number, Type::Number], Box::new(Type::Number)),
        "setDate" => Type::Function(vec![Type::Number], Box::new(Type::Number)),
        "setHours" => Type::Function(
            vec![Type::Number, Type::Number, Type::Number, Type::Number],
            Box::new(Type::Number),
        ),
        "setMinutes" => Type::Function(
            vec![Type::Number, Type::Number, Type::Number],
            Box::new(Type::Number),
        ),
        "setSeconds" => Type::Function(vec![Type::Number, Type::Number], Box::new(Type::Number)),
        "setMilliseconds" => Type::Function(vec![Type::Number], Box::new(Type::Number)),
        "getYear" => Type::Function(Vec::new(), Box::new(Type::Number)),
        "toGMTString" | "toUTCString" | "toDateString" | "toLocaleString"
        | "toLocaleDateString" | "toLocaleTimeString" | "toString" => {
            Type::Function(Vec::new(), Box::new(Type::String))
        }
        _ => return None,
    };
    Some(Ok(ty))
}
