//! The global-name table backing [`super::free_vars`] — split to its
//! own file when rotation 346's function-this boundary work pushed
//! the walker past the 500-line hard limit.

/// The names the walk treats as globals rather than captures: a
/// reference to one inside an arrow body resolves through the
/// checker's ident fallback, and capturing it would rename it out
/// from under that resolution. An enclosing scope that REBINDS one
/// (`const fs = […]`, `let process = …`) makes it an ordinary local
/// again — `lift_arrow_fns` asks `scope_chain` and hands the
/// walk those names as `Bound::rebound_globals`.
pub(crate) const GLOBAL_NAMES: &[&str] = &[
    // Built-in objects + namespaces
    "console",
    "Math",
    "JSON",
    "Object",
    "Array",
    "String",
    "Number",
    "Boolean",
    "Symbol",
    "Date",
    "RegExp",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "Promise",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Proxy",
    "Reflect",
    "BigInt",
    "ArrayBuffer",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "Float16Array",
    "DataView",
    "Function",
    // §27.1.3 Iterator global (RFC 20260730-iterator-global
    // 刀 1) — landed in the checker fallback but this table
    // drifted: a closure body's `Iterator.concat(...)`
    // collected `Iterator` as a capture and the rename broke
    // both the checker resolution and the statics wedge.
    "Iterator",
    "WeakRef",
    "FinalizationRegistry",
    // Runtime module namespaces the checker fallback types
    // (kept in the same sync).
    "fs",
    "fs_promises",
    "process",
    "Bun",
    "performance",
    // Numeric constants
    "NaN",
    "Infinity",
    "undefined",
    // §19.2.1 — the global `eval` as a VALUE resolves through
    // the checker fallback (Type::Any) exactly like
    // `globalThis`; collecting it as a capture renames it out
    // from under that resolution (the Iterator drift, one row
    // down, was the same failure).
    "eval",
    // Top-level coercion functions
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURI",
    "decodeURI",
    "encodeURIComponent",
    "decodeURIComponent",
    // §B.2.1 — the two Annex B string globals.
    "escape",
    "unescape",
    "globalThis",
    // WHATWG HTML microtask scheduling (P10.1)
    "queueMicrotask",
];

pub(crate) fn is_global_name(name: &str) -> bool {
    // Compiler-synthesized helpers (`__torajs_date_from_ms`, the
    // arguments materializer, reify hooks, ...) resolve through the
    // checker's ident fallback and lower as intrinsics — capturing
    // one renames it out from under that resolution (a lifted
    // closure inside desugared Date code answered "references
    // unknown identifier `__torajs_date_from_ms`").
    name.starts_with("__torajs_") || GLOBAL_NAMES.contains(&name)
}
