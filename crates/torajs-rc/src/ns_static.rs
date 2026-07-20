//! Namespace-static intern table (RFC 20260719-ns-static-value-reify)
//! — the shared compile-time/runtime truth for builtin namespace
//! statics read as VALUES (`const m = Math.max`). The compiler
//! resolves `(namespace, member)` to an id at lower time and bakes
//! `__torajs_ns_static_value(id)` calls; torajs-anyvalue's minted
//! cells carry the id and dispatch/reflect through [`ns_static_meta`].
//!
//! Append-only: ids are table indices — extending the surface means
//! pushing rows at the END (a reorder would silently re-key every
//! baked call site). torajs-anyvalue's dispatch table asserts
//! same-length lockstep in its unit tests.

/// Miss sentinel for [`ns_static_id`].
pub const NS_STATIC_UNKNOWN: i64 = -1;

/// One namespace-static row — `length` is the ES-spec function
/// `length` (`Math.max.length` is 2 per §21.3.2.25).
pub struct NsStaticRow {
    pub ns: &'static str,
    pub name: &'static str,
    pub length: u32,
}

const fn row(ns: &'static str, name: &'static str, length: u32) -> NsStaticRow {
    NsStaticRow { ns, name, length }
}

/// Id = index. Math family first (chunk B1); console / JSON and the
/// remaining namespaces append behind (RFC chunks B2/B3).
pub static NS_STATIC_TABLE: &[NsStaticRow] = &[
    row("Math", "sqrt", 1),
    row("Math", "abs", 1),
    row("Math", "floor", 1),
    row("Math", "ceil", 1),
    row("Math", "log", 1),
    row("Math", "exp", 1),
    row("Math", "sign", 1),
    row("Math", "round", 1),
    row("Math", "trunc", 1),
    row("Math", "sin", 1),
    row("Math", "cos", 1),
    row("Math", "tan", 1),
    row("Math", "asin", 1),
    row("Math", "acos", 1),
    row("Math", "atan", 1),
    row("Math", "log2", 1),
    row("Math", "log10", 1),
    row("Math", "cbrt", 1),
    row("Math", "sinh", 1),
    row("Math", "cosh", 1),
    row("Math", "tanh", 1),
    row("Math", "asinh", 1),
    row("Math", "acosh", 1),
    row("Math", "atanh", 1),
    row("Math", "expm1", 1),
    row("Math", "log1p", 1),
    row("Math", "fround", 1),
    row("Math", "f16round", 1),
    row("Math", "pow", 2),
    row("Math", "min", 2),
    row("Math", "max", 2),
    row("Math", "atan2", 2),
    row("Math", "imul", 2),
    row("Math", "clz32", 1),
    row("Math", "random", 0),
    // console stdout family (chunk B2) — WHATWG console §1.1: the
    // methods are rest-param shaped, ES length 0 (bun agrees).
    // error / warn stay OFF the table until an any-print stderr
    // kernel exists (RFC records the face) — their value reads keep
    // the loud unknown-ident reject.
    row("console", "log", 0),
    row("console", "info", 0),
    row("console", "debug", 0),
    // Number / Array / Object statics (chunk B3a) — the predicate
    // family computes inline on the NaN-box (spec: no coercion),
    // parseInt/parseFloat delegate to the typed tier's
    // __torajs_num_parse_* kernels, Object.is to the §7.2.10
    // same-value kernel. Lengths per §21.1.2.13/.12 / §23.1.2.2 /
    // §20.1.2.14.
    row("Number", "parseInt", 2),
    row("Number", "parseFloat", 1),
    row("Number", "isInteger", 1),
    row("Number", "isNaN", 1),
    row("Number", "isFinite", 1),
    row("Number", "isSafeInteger", 1),
    row("Array", "isArray", 1),
    row("Object", "is", 2),
    // Object statics (chunk B3c-1) — the family whose semantics
    // already exist as AnyValue-tier runtime kernels, so every arm
    // delegates rather than re-deriving §20.1.2. Lengths per
    // §20.1.2.{17,23,5,1,6,13,12,21,7}. `create` /
    // `defineProperty` / `defineProperties` stay OFF the table:
    // their kernels are dynobj-slot shaped (ptr-to-ptr receiver, no
    // AnyValue entry), so a value read keeps the loud reject until
    // an any-tier kernel exists (RFC records the face).
    row("Object", "keys", 1),
    row("Object", "values", 1),
    row("Object", "entries", 1),
    row("Object", "assign", 2),
    row("Object", "freeze", 1),
    row("Object", "isFrozen", 1),
    row("Object", "getPrototypeOf", 1),
    row("Object", "setPrototypeOf", 2),
    row("Object", "fromEntries", 1),
    // Symbol registry pair (chunk B3c-2) — §20.4.2.2/.6. The other
    // Symbol statics are well-known-symbol DATA properties
    // (`Symbol.iterator` etc.), not functions, so this table — a
    // function-cell intern — is not their surface.
    row("Symbol", "for", 1),
    row("Symbol", "keyFor", 1),
    // Ctor statics batch 1 (RFC 20260720-ctor-static-reflection 刀 1)
    // — the family whose AnyValue-tier kernels already exist (Date
    // wall-clock/parse/UTC, the per-code Str mints, the prop_has
    // probe). Lengths per §21.4.3.{1,2,4} / §22.1.2.{1,2} /
    // §20.1.2.11. `Array.from` stays OFF the table: full §23.1.2.1
    // needs the iterator protocol + mapFn over any source — no
    // AnyValue kernel yet, so its value read keeps the loud reject
    // (RFC records the face).
    row("Date", "now", 0),
    row("Date", "parse", 1),
    row("Date", "UTC", 7),
    row("String", "fromCharCode", 1),
    row("String", "fromCodePoint", 1),
    row("Object", "hasOwn", 2),
];

/// Compile-time `(namespace, member)` → id. Linear scan — lower-time
/// only, never on a runtime path.
pub fn ns_static_id(ns: &str, name: &str) -> i64 {
    NS_STATIC_TABLE
        .iter()
        .position(|r| r.ns == ns && r.name == name)
        .map(|i| i as i64)
        .unwrap_or(NS_STATIC_UNKNOWN)
}

/// Runtime id → row (name for `[Function: <name>]` / `.name`,
/// length for `.length`).
pub fn ns_static_meta(id: i64) -> Option<&'static NsStaticRow> {
    if id < 0 {
        return None;
    }
    NS_STATIC_TABLE.get(id as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_meta_roundtrip() {
        for (i, r) in NS_STATIC_TABLE.iter().enumerate() {
            assert_eq!(
                ns_static_id(r.ns, r.name),
                i as i64,
                "row {}.{}",
                r.ns,
                r.name
            );
            let m = ns_static_meta(i as i64).unwrap();
            assert_eq!((m.ns, m.name), (r.ns, r.name));
        }
    }

    #[test]
    fn miss_is_unknown() {
        assert_eq!(ns_static_id("Math", "sumPrecise"), NS_STATIC_UNKNOWN);
        assert_eq!(ns_static_id("Nope", "max"), NS_STATIC_UNKNOWN);
        assert!(ns_static_meta(NS_STATIC_UNKNOWN).is_none());
        assert!(ns_static_meta(NS_STATIC_TABLE.len() as i64).is_none());
    }

    #[test]
    fn spec_lengths() {
        assert_eq!(
            ns_static_meta(ns_static_id("Math", "max")).unwrap().length,
            2
        );
        assert_eq!(
            ns_static_meta(ns_static_id("Math", "sqrt")).unwrap().length,
            1
        );
        assert_eq!(
            ns_static_meta(ns_static_id("Math", "random"))
                .unwrap()
                .length,
            0
        );
    }
}
