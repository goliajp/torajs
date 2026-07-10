//! `Type::String` instance-method arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 197 — seventh sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-196 try_match shape).
//!
//! Covers the single-`Type::String` arms (18 arm groups / ~38
//! method names: slice / substring / substr / repeat / trim*
//! family + normalize + toWellFormed + isWellFormed / padStart /
//! padEnd / replace / replaceAll / charAt / at /
//! toLocaleLowerCase / toLocaleUpperCase / valueOf / toString /
//! toLocaleString / charCodeAt / codePointAt / startsWith /
//! endsWith / includes / indexOf / lastIndexOf / localeCompare /
//! search / split / match / matchAll / concat).
//!
//! Mixed-type arms stay in the main match:
//! - `(Type::String, "length") | (Type::Array(_), "length")`
//!   (String + Array union)
//! - `(Type::Number | Type::String | …, "constructor")`
//!   (primitive-wrapper union, V3-18 m2.c)
//! - `(Type::Number | Type::String | … | Type::Any,
//!   "hasOwnProperty" | "propertyIsEnumerable")`
//!   (primitive + Any union, V3-18 m2.a)
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // M6.1 — String methods. All borrow `this` and any
        // String args (consumption only fires at concat,
        // which has its own arm). Bool-returning methods
        // return Type::Boolean; index/charCodeAt return
        // Number; slice returns String.
        (Type::String, "slice" | "substring") => {
            Type::Function(vec![Type::Number, Type::Number], Box::new(Type::String))
        }
        // T-49 — `String.prototype.substr(start, length?)` (annexB
        // legacy). The 1-arg shape is the common one in test262;
        // the call-site arity-tolerance arm above
        // (`slice / substring / substr` with args.len() < 2)
        // accepts 0/1 args, and ssa_lower fills the missing
        // length with i64::MAX so the runtime helper clamps.
        (Type::String, "substr") => {
            Type::Function(vec![Type::Number, Type::Number], Box::new(Type::String))
        }
        (Type::String, "repeat") => {
            Type::Function(vec![Type::Number], Box::new(Type::String))
        }
        (
            Type::String,
            "toUpperCase"
            | "toLowerCase"
            | "trim"
            | "trimStart"
            | "trimEnd"
            // `trimLeft` / `trimRight` are the non-standard but
            // de-facto aliases that ship in every JS engine —
            // ECMAScript Annex B documents them as legacy of
            // `trimStart` / `trimEnd`.
            | "trimLeft"
            | "trimRight"
            // s.normalize() — Unicode normalization. tr's
            // current Str layer is byte-oriented; for ASCII
            // strings (the dominant test262 case) all four NFC/
            // NFD/NFKC/NFKD forms are byte-identical with the
            // input, so an identity stub round-trips correctly.
            // Multi-byte UTF-8 strings would need Unicode tables
            // — deferred to v1.0 (`\p{...}` + ICU work).
            | "normalize"
            // ES2024 §22.1.3.30 — `toWellFormed()`. torajs is
            // internally UTF-8, so lone surrogates can't be
            // encoded; identity stub at lower time.
            | "toWellFormed",
        ) => Type::Function(Vec::new(), Box::new(Type::String)),
        // ES2024 §22.1.3.10 — `isWellFormed()`. Mirror of
        // toWellFormed (see above) — always true.
        (Type::String, "isWellFormed") => {
            Type::Function(Vec::new(), Box::new(Type::Boolean))
        }
        (Type::String, "padStart" | "padEnd") => {
            Type::Function(vec![Type::Number, Type::String], Box::new(Type::String))
        }
        (Type::String, "replace" | "replaceAll") => {
            // Pattern arg is either a literal Str (existing
            // string-only path through __torajs_str_replace
            // / __torajs_str_replace_all) or a RegExp
            // (Phase 1b regex path). Repl arg is either a
            // Str (existing path) or a callback fn (P9.5).
            // Both args use Type::Any here so each can pass
            // typecheck; ssa_lower picks the dispatch by
            // operand SSA type. A1 callback shape required:
            // `(m: string) => string` — multi-arg / capture-
            // spread callbacks are A1.1.
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::String))
        }
        // `s.charAt(i)` — single-char substring at index i.
        // Identical surface to `s[i]`; routed through the
        // same substr_create / substr_slice path at lower
        // time. tr's subset doesn't return "" on OOB —
        // matches the unchecked-index convention used by
        // index access.
        (Type::String, "charAt") => {
            Type::Function(vec![Type::Number], Box::new(Type::String))
        }
        (Type::String, "at") => {
            Type::Function(vec![Type::Number], Box::new(Type::String))
        }
        // S140 — `s.toLocaleLowerCase` / `toLocaleUpperCase`
        // per ES §22.1.3.21 / §22.1.3.23 accept optional
        // locales arg; tr's subset is en-US only so the
        // arg is accepted (Any?) and ignored — same shape
        // as Number.toLocaleString (S139).
        (Type::String, "toLocaleLowerCase" | "toLocaleUpperCase") => {
            Type::Function(vec![Type::Any], Box::new(Type::String))
        }
        // V3-18 wedge — String.prototype.toString /
        // toLocaleString / valueOf all return the
        // primitive string itchecker per JS spec
        // §22.1.3.27 / §22.1.3.31 / §22.1.3.34.
        // Already wired for Number / Boolean / BigInt /
        // Symbol but missing for String, so `s.toString()`
        // hit 'no member .toString on type String'.
        (Type::String, "valueOf" | "toString" | "toLocaleString") => {
            Type::Function(Vec::new(), Box::new(Type::String))
        }
        (Type::String, "charCodeAt" | "codePointAt") => {
            Type::Function(vec![Type::Number], Box::new(Type::Number))
        }
        (Type::String, "startsWith" | "endsWith" | "includes") => {
            Type::Function(vec![Type::String], Box::new(Type::Boolean))
        }
        (
            Type::String,
            "indexOf"
            | "lastIndexOf"
            | "localeCompare"
            // V3-18 wedge — String.prototype.search per JS
            // spec §22.1.3.16. The full spec coerces the
            // arg to a RegExp and uses Symbol.search, but
            // for a plain string arg the result is exactly
            // indexOf — first match position or -1. This
            // entry types the string-arg form only; the
            // RegExp-arg form routes through the chunk-800
            // `check_type_of_call_string_search_regex`
            // wedge before the general arm reaches here.
            | "search",
        ) => Type::Function(vec![Type::String], Box::new(Type::Number)),
        // s.split(sep): string[] — `sep` is Str or RegExp;
        // Type::Any lets either type pass typecheck and
        // ssa_lower dispatches on operand SSA type to the
        // string-only `__torajs_str_split` or the regex
        // path `__torajs_str_split_regex`.
        // V3-18 wedge — `s.split(sep [, limit])` per ES
        // §22.1.3.21. The optional `limit` slot uses
        // Type::Any so the trailing-Any arity-pad path
        // makes 1-arg calls type-check; the SSA layer in
        // `ssa_lower_str.rs` already branches on
        // `args.len() == 2` to emit the slice clamp.
        (Type::String, "split") => Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Array(Box::new(Type::String))),
        ),
        // s.match(re) — Phase 1b returns Array<Str>; without
        // `g` flag the array has 1 element (the matched
        // substring), with `g` it has all matches. Capture
        // groups + JS-spec null-on-miss are Phase 1c.
        (Type::String, "match") => Type::Function(
            vec![Type::RegExp],
            Box::new(Type::Array(Box::new(Type::String))),
        ),
        // s.matchAll(re) — Phase 1c.3 returns
        // Array<Array<Str>>: outer = one entry per match,
        // each inner = exec-shape [match, g1, g2, ...].
        // JS spec returns an iterator; tr's array stand-in
        // covers the dominant test262 usage pattern (a for-of
        // loop or [...m]) until iterator protocol lands.
        (Type::String, "matchAll") => Type::Function(
            vec![Type::RegExp],
            Box::new(Type::Array(Box::new(Type::Array(Box::new(Type::String))))),
        ),
        // `s.concat(other)` — string concat. The single-arg
        // shape lives here so the standard method-call path
        // typechecks normally. Variadic forms drop into the
        // arity-≠-1 guard below the Math/String variadic
        // block.
        (Type::String, "concat") => {
            Type::Function(vec![Type::String], Box::new(Type::String))
        }
        _ => return None,
    };
    Some(Ok(ty))
}
