//! Primitive-wrapper instance-method arms (Number / Boolean /
//! BigInt / Symbol) extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 194 — fourth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-193 try_match shape).
//!
//! Covers only **single-primitive arms** (those whose pattern
//! matches a single `Type::Primitive` variant). Mixed
//! arms — e.g. `(Type::Number | Type::String | Type::Boolean |
//! Type::BigInt | Type::Symbol, "constructor")` or the multi-
//! type `hasOwnProperty` / `propertyIsEnumerable` block — stay
//! in the main match so the String/Any branches are still
//! served when this dispatch returns None.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // Number numeric formatters per ES §21.1.3.{3,4,5}.
        (Type::Number, "toFixed" | "toExponential" | "toPrecision") => {
            Type::Function(vec![Type::Number], Box::new(Type::String))
        }
        (Type::Number, "toString") => Type::Function(Vec::new(), Box::new(Type::String)),
        // S139 — `n.toLocaleString(locales?, options?)` per ES
        // §21.1.3.4 accepts optional locale + options args;
        // tr's subset is en-US only and ignores them, so the
        // signature is `(Any?, Any?) -> string`. ssa_lower
        // drops the args before the 1-arg runtime helper.
        (Type::Number, "toLocaleString") => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::String))
        }
        // V3-18 m2.a — Object.prototype methods exposed on
        // every primitive via JS's auto-boxing rules:
        //   .valueOf()              → returns the primitive itself
        //   .hasOwnProperty(name)   → false (primitives have
        //                             no own properties in our
        //                             subset)
        //   .propertyIsEnumerable(name) → false (same)
        //   .isPrototypeOf(obj)     → false (we have no real
        //                             prototype chain)
        // ssa_lower handles the dispatch with constant folds
        // since the values can't actually carry user-added
        // properties.
        (Type::Number, "valueOf") => Type::Function(Vec::new(), Box::new(Type::Number)),
        // V3-18 wedge — Boolean.prototype.toString / valueOf.
        // Per JS spec §20.3.3.2 / §20.3.3.3 — `(true).toString()`
        // → "true", `(false).toString()` → "false". valueOf
        // returns the boolean itchecker. Common in calls like
        // `b.toString()` where b is a typed Boolean binding.
        (Type::Boolean, "toString" | "toLocaleString") => {
            Type::Function(Vec::new(), Box::new(Type::String))
        }
        (Type::Boolean, "valueOf") => Type::Function(Vec::new(), Box::new(Type::Boolean)),
        // V3-18 m1.h.27 — BigInt.prototype.toString() →
        // decimal string (no `n` suffix). Per JS spec
        // §21.2.3.5 / §21.2.3.6. The runtime path
        // already exists (used by string concat coerce);
        // this just wires up the typecheck.
        //
        // P12.4 — accept optional radix argument per ES
        // §6.1.6.2.13 (`BigInt.prototype.toString(radix)`).
        // Signature is `(radix?: Any) → String`; the Any
        // slot is padded to `undefined` for the 0-arg
        // common path and accepts a Number radix when
        // supplied (ssa-lower routes to bigint_to_string
        // vs bigint_to_string_radix per arg count).
        // toLocaleString remains 0-arg per ES §22.2.3.2.
        (Type::BigInt, "toString") => Type::Function(vec![Type::Any], Box::new(Type::String)),
        (Type::BigInt, "toLocaleString") => Type::Function(Vec::new(), Box::new(Type::String)),
        (Type::BigInt, "valueOf") => Type::Function(Vec::new(), Box::new(Type::BigInt)),
        // V3-18 m1.h.47 — Symbol.prototype.toString() →
        // "Symbol(<desc>)" / "Symbol()". Symbol.description
        // returns the desc (or null for Symbol() with no
        // arg). Per JS spec §20.4.3.3 / §20.4.3.2.
        (Type::Symbol, "toString" | "toLocaleString") => {
            Type::Function(Vec::new(), Box::new(Type::String))
        }
        // V3-18 m1.h.47 — Symbol.prototype.description.
        // Returns the desc the Symbol was created with, or
        // null if Symbol() was called with no arg. Per JS
        // spec §20.4.3.2.
        (Type::Symbol, "description") => Type::String,
        _ => return None,
    };
    Some(Ok(ty))
}
