//! `Type::Object("NAMESPACE")` static-namespace arms extracted
//! from [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 200 — tenth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-199 try_match shape).
//!
//! Covers the **single-namespace** arms (those whose pattern is
//! a single `Type::Object("X")`):
//! - `console` (log / error / warn / info / debug)
//! - `Math` (~30 unary + binary + sumPrecise + random + constants)
//! - `Number` (constants + parseInt / parseFloat + is* predicates)
//! - `BigInt` (asIntN / asUintN)
//! - `JSON` (stringify / parse)
//! - `Array` static (isArray / from)
//! - `Object` static — only the **single-`Type::Object("Object")`**
//!   arms (is / entries / fromEntries / values / freeze /
//!   isFrozen); mixed arms with Reflect / generic Object(_)
//!   stay in the main match.
//! - `Promise` static (resolve / reject)
//! - `Symbol` static (for / keyFor)
//!
//! Mixed-namespace arms stay in the main match:
//! - `(Type::Object("Object"), "keys") | … |
//!   (Type::Object("Reflect"), "ownKeys")` — Object ∪ Reflect
//! - `(Type::Object("Object"), "hasOwn") |
//!   (Type::Object("Reflect"), "has")` — Object ∪ Reflect
//! - `(Type::Object(_), "hasOwnProperty" / "propertyIsEnumerable"
//!   / "isPrototypeOf" / "toString" / "prototype" / "name" /
//!   "length")` — generic Object(_) catch-alls
//! - Object accessor / defineProperty / setPrototypeOf /
//!   getPrototypeOf / getOwnPropertyDescriptor /
//!   defineProperties / create / assign / preventExtensions /
//!   seal / isExtensible / isSealed — these involve mutation /
//!   accessor semantics that share lowering paths
//!
//! Date / String static (fromCharCode) / Bun / BunFile /
//! Response / process / env / fs / fs_promises namespace arms
//! land in a follow-up chunk (the I/O-flavored namespaces).
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // S328 — WHATWG console §1.1.{2,4}: `info` /
        // `debug` print to the same stream as `log`.
        // bun aliases info/debug to log (stdout); tr
        // routes through the same `print_*` intrinsic
        // family in ssa_lower.
        (Type::Object("console"), "log" | "error" | "warn" | "info" | "debug") => {
            Type::Function(vec![Type::Any], Box::new(Type::Void))
        }
        // `Math` global — every method takes one number and
        // returns a number. f64-flavored at the SSA level
        // (the lowerer auto-promotes integer args), but
        // check.rs uses the umbrella Type::Number.
        (
            Type::Object("Math"),
            "sqrt" | "abs" | "floor" | "ceil" | "log" | "exp" | "sign" | "round" | "trunc" | "sin"
            | "cos" | "tan" | "asin" | "acos" | "atan" | "log2" | "log10" | "cbrt" | "sinh"
            | "cosh" | "tanh" | "asinh" | "acosh" | "atanh" | "expm1" | "log1p" | "clz32"
            | "fround" | "f16round",
        ) => Type::Function(vec![Type::Number], Box::new(Type::Number)),
        (Type::Object("Math"), "imul") => {
            Type::Function(vec![Type::Number, Type::Number], Box::new(Type::Number))
        }
        // ES2025 §21.3.2.32 — correctly-rounded sum of an
        // Array<Number>. Narrow form: only Array<Number>
        // input (spec accepts any iterable of Number;
        // tora's Set/Map/iterator surface comes later).
        (Type::Object("Math"), "sumPrecise") => Type::Function(
            vec![Type::Array(Box::new(Type::Number))],
            Box::new(Type::Number),
        ),
        (Type::Object("Math"), "random") => Type::Function(Vec::new(), Box::new(Type::Number)),
        // Two-arg methods: pow(x, y), min(a, b), max(a, b),
        // atan2(y, x).
        (Type::Object("Math"), "pow" | "min" | "max" | "atan2") => {
            Type::Function(vec![Type::Number, Type::Number], Box::new(Type::Number))
        }
        // Constants — read directly without parens.
        (
            Type::Object("Math"),
            "PI" | "E" | "LN2" | "LN10" | "LOG2E" | "LOG10E" | "SQRT2" | "SQRT1_2",
        ) => Type::Number,
        // Number namespace constants — common floating-point
        // limits and integer-safety bounds.
        (
            Type::Object("Number"),
            "NaN" | "POSITIVE_INFINITY" | "NEGATIVE_INFINITY" | "EPSILON" | "MAX_SAFE_INTEGER"
            | "MIN_SAFE_INTEGER" | "MAX_VALUE" | "MIN_VALUE",
        ) => Type::Number,
        // `Number` global — parseInt / parseFloat coerce a
        // string to a number; isInteger / isNaN / isFinite
        // are unary number predicates.
        (Type::Object("Number"), "parseInt") => {
            Type::Function(vec![Type::String, Type::Number], Box::new(Type::Number))
        }
        (Type::Object("Number"), "parseFloat") => {
            Type::Function(vec![Type::String], Box::new(Type::Number))
        }
        (Type::Object("Number"), "isInteger" | "isNaN" | "isFinite" | "isSafeInteger") => {
            Type::Function(vec![Type::Number], Box::new(Type::Boolean))
        }
        // P12.4-B/C — `BigInt.asIntN(bits, value)` /
        // `BigInt.asUintN(bits, value)` per ES §21.2.2.1 /
        // §21.2.2.2. `bits` is a `Number` (Index per spec;
        // tora's `Number` covers integer-shaped values
        // already); `value` is `BigInt`; returns BigInt.
        (Type::Object("BigInt"), "asIntN" | "asUintN") => {
            Type::Function(vec![Type::Number, Type::BigInt], Box::new(Type::BigInt))
        }
        // JSON.stringify(value) — value can be any subset
        // type; result is String. The actual type-aware
        // serialization shape happens at lower-time
        // (per-call-site monomorphization).
        (Type::Object("JSON"), "stringify") => {
            Type::Function(vec![Type::Any], Box::new(Type::String))
        }
        // M6.3 — `JSON.parse(text): T` — caller-driven type
        // inference. The return type at typecheck level is
        // Any (effectively a hole); ssa_lower's LetDecl
        // arm reads the slot's `type_ann` and emits the
        // per-shape parser at lower time. check.rs accepts
        // any `Type::Any` slot, so the let binding's
        // declared `T` slot type drives the actual decode.
        (Type::Object("JSON"), "parse") => Type::Function(vec![Type::String], Box::new(Type::Any)),
        // Array.isArray(x) — compile-time static check.
        (Type::Object("Array"), "isArray") => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        // `Array.from(s)` over a string — returns `string[]`
        // with one single-char string per byte. The other
        // overloads (iterable / arrayLike / mapFn) aren't in
        // tr's subset; ssa_lower validates the arg is Type::Str
        // at lower-time.
        (Type::Object("Array"), "from") => Type::Function(
            vec![Type::String],
            Box::new(Type::Array(Box::new(Type::String))),
        ),
        // Object.is(a, b) — strict equality with two
        // corner-case overrides vs `===`: NaN is equal to
        // NaN, and +0 is NOT equal to -0. Lowered per arg
        // SSA type (Type::Number → __torajs_object_is_f64
        // runtime helper that bitcasts the ±0 case;
        // Type::String → __torajs_str_eq; everything else
        // falls back to SSA-level == compare).
        (Type::Object("Object"), "is") => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Boolean))
        }
        /* T-09.b (v0.4.0) — Object.entries(obj) returns
         * `Array<Array<Any>>` (each inner is `[key, value]`).
         * Codegen unfolds at compile time using the static
         * struct layout from check.rs's struct_layouts —
         * zero-cost reflection just like Object.keys. The
         * Type::Any tagged-slot path from T-10 carries the
         * mixed key (Str) + value (per-field type). */
        (Type::Object("Object"), "entries") => Type::Function(
            vec![Type::Any],
            Box::new(Type::Array(Box::new(Type::Array(Box::new(Type::Any))))),
        ),
        /* T-09.c (v0.4.0) — Object.fromEntries(entries)
         * uses caller-driven typing (similar to JSON.parse):
         * the typecheck-level return is Any, and ssa_lower's
         * LetDecl arm unfolds per the slot struct schema.
         * MVP: entries are assumed to be in struct field
         * declaration order (matches Object.entries
         * round-trip), no key-matching scan. */
        (Type::Object("Object"), "fromEntries") => Type::Function(
            vec![Type::Array(Box::new(Type::Array(Box::new(Type::Any))))],
            Box::new(Type::Any),
        ),
        /* S258 — Object.values(obj) → Array<Any>. SSA-emit
         * already dispatches Obj/Arr/Str/Any receivers
         * (ssa_lower.rs ~18495); checktime sig was missing.
         * Return Array<Any> — heterogeneous struct fields
         * + Any receiver both box to Any per ssa_lower's
         * anyv_struct_values walker; homogeneous struct
         * + Arr receivers also typecheck under Array<Any>
         * (downcast-on-use). */
        (Type::Object("Object"), "values") => {
            Type::Function(vec![Type::Any], Box::new(Type::Array(Box::new(Type::Any))))
        }
        /* T-09.d (v0.4.0) — Object.freeze(obj) sets the
         * FROZEN bit on the universal heap header. Returns
         * the same obj per spec. Subsequent field writes
         * are silently ignored (matches non-strict mode;
         * tr has no `"use strict"` directive). The arg
         * type is permissive (Type::Any) — runtime accepts
         * any heap object pointer. */
        (Type::Object("Object"), "freeze") => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        /* Object.isFrozen(obj) — reads the FROZEN bit. */
        (Type::Object("Object"), "isFrozen") => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        /* T-15.g.1 — Promise.resolve(v) / Promise.reject(v).
         * MVP only Number arg (Type::Promise<Number>);
         * heap types (Promise<string>, etc.) land in
         * T-15.g.4 via direct call-arm handling that
         * inspects the inferred arg type at the call site
         * (the static-method table's TypeVar isn't
         * instantiated automatically). */
        (Type::Object("Promise"), "resolve" | "reject") => Type::Function(
            vec![Type::Number],
            Box::new(Type::Promise(Box::new(Type::Number))),
        ),
        /* T-13.b (v0.4.0) — Symbol.for(key) returns the
         * registered Symbol for the key (creates one on
         * first call). Identity preserved across calls. */
        (Type::Object("Symbol"), "for") => {
            Type::Function(vec![Type::String], Box::new(Type::Symbol))
        }
        /* Symbol.keyFor(s) — inverse: returns the key
         * Symbol.for() registered the symbol under, or
         * null for unregistered (Symbol(...)) symbols. */
        (Type::Object("Symbol"), "keyFor") => Type::Function(
            vec![Type::Symbol],
            Box::new(Type::Nullable(Box::new(Type::String))),
        ),
        _ => return None,
    };
    let _ = obj_ty;
    Some(Ok(ty))
}
