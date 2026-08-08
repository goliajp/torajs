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
    // §20.1.2.11. (`Array.from` joined the table in batch 6 for the
    // REFLECTION surface only — see that batch's note.)
    row("Date", "now", 0),
    row("Date", "parse", 1),
    row("Date", "UTC", 7),
    row("String", "fromCharCode", 1),
    row("String", "fromCodePoint", 1),
    row("Object", "hasOwn", 2),
    // Ctor statics batch 2 (RFC 20260720 刀 4) — the Object
    // integrity family whose AnyValue kernels shipped with RFC C5b
    // (`__torajs_anyv_{prevent_extensions,is_extensible,seal,
    // is_sealed}`) plus the own-keys nonenum surface. Lengths per
    // §20.1.2.{10,16,13,20,15}.
    row("Object", "getOwnPropertyNames", 1),
    row("Object", "preventExtensions", 1),
    row("Object", "isExtensible", 1),
    row("Object", "seal", 1),
    row("Object", "isSealed", 1),
    // Ctor statics batch 3 (RFC 20260720 刀 5b-2) — the fixed-width
    // BigInt views, backed by the arbitrary-bits kernels (刀 5a) +
    // the any-lane ToBigInt coercion (torajs-anyvalue to_bigint.rs).
    // Lengths per §21.2.2.{1,2}.
    row("BigInt", "asIntN", 2),
    row("BigInt", "asUintN", 2),
    // Ctor statics batch 4 (RFC 20260720 刀 6) — the Promise settle
    // pair: thenable absorption via the existing resolve_thenable
    // kernel, everything else through the REPR_ANY-stamped any
    // allocs. Lengths per §27.2.4.{7,6}.
    row("Promise", "resolve", 1),
    row("Promise", "reject", 1),
    // Ctor statics batch 5 (RFC 20260721 刀 4) — the descriptor
    // family reified for the REFLECTION surface (typeof / name /
    // length / gOPD identity). gOPD settles through the meta
    // descriptor kernel; create / defineProperty / defineProperties
    // / gOPDs raise the recorded dynobj-slot-writeback TypeError on
    // a detached call (any-tier define kernel is the RFC face).
    // Lengths per §20.1.2.{8,9,2,4,3}.
    row("Object", "getOwnPropertyDescriptor", 2),
    row("Object", "getOwnPropertyDescriptors", 1),
    row("Object", "create", 2),
    row("Object", "defineProperty", 3),
    row("Object", "defineProperties", 2),
    // Ctor statics batch 6 (RFC 20260721-builtin-method-reflection
    // 刀 3). gOPS delegates the W-N-c truth (tr has no symbol-keyed
    // props → every own-symbol list is empty; nullish ToObject still
    // throws) to the same kernel the typed call lane uses.
    // `Array.from` is reified for the REFLECTION surface only
    // (typeof / name / length / gOPD identity): full §23.1.2.1 needs
    // the iterator protocol + mapFn over any source — no AnyValue
    // kernel yet, so a detached CALL raises the recorded loud
    // TypeError (the batch-5 define-family posture). Lengths per
    // §20.1.2.10 / §23.1.2.1.
    row("Object", "getOwnPropertySymbols", 1),
    row("Array", "from", 1),
    // §27.2.4.{1,3,5,6} Promise combinator statics — reflection rows
    // (RFC 20260722-builtin-proto-reflection 刀 1). The direct call
    // form lowers through promise_*_sync intrinsics; a bare cell
    // call raises the spec step-2 "|this| is not an object"
    // TypeError, same as resolve / reject (Disp::PromiseSettle).
    row("Promise", "all", 1),
    row("Promise", "allSettled", 1),
    row("Promise", "any", 1),
    row("Promise", "race", 1),
    // Iterator statics (RFC 20260731 刀 5 — the concat/zip reflection
    // faces). Lengths per proposal-iterator-sequencing (`concat` is
    // all-rest → 0) and proposal-joint-iteration (`zip` / `zipKeyed`
    // take one required iterables argument; options is optional).
    row("Iterator", "from", 1),
    row("Iterator", "concat", 0),
    row("Iterator", "zip", 1),
    row("Iterator", "zipKeyed", 1),
    // §28.1.5 Reflect.getOwnPropertyDescriptor (rotation 266 刀 R1)
    // — strict IsObject gate (no ToObject) in front of the same meta
    // descriptor kernel Object.getOwnPropertyDescriptor settles
    // through. Length per §28.1.5.
    row("Reflect", "getOwnPropertyDescriptor", 2),
    // §28.1.{4,8,10} Reflect read-only trio (rotation 266 刀 R2) —
    // same strict IsObject gate in front of the Object-flavor
    // kernels. preventExtensions answers boolean true (ordinary
    // [[PreventExtensions]] always succeeds) instead of the
    // receiver pass-through. Lengths per spec.
    row("Reflect", "getPrototypeOf", 1),
    row("Reflect", "preventExtensions", 1),
    row("Reflect", "isExtensible", 1),
    // §28.1.3 Reflect.deleteProperty (rotation 266 刀 R3) — strict
    // gate + the OrdinaryDelete kernel. Length per spec.
    row("Reflect", "deleteProperty", 2),
    // §28.1.12 Reflect.setPrototypeOf (rotation 266 刀 R4) — strict
    // gate + the boolean-answer OrdinarySetPrototypeOf core.
    row("Reflect", "setPrototypeOf", 2),
    // §23.1.2.3 Array.of — the call face packs argv into a fresh
    // Array<Any> (the direct-call form lowers through the
    // array-literal wedge). Length 0 per spec (rest-param shaped).
    row("Array", "of", 0),
    // ES2025 §22.2.5.1 RegExp.escape — strict String gate (no
    // ToString) + the EncodeForRegExpEscape kernel. Length 1.
    row("RegExp", "escape", 1),
    // §28.1.2 Reflect.defineProperty (rotation 267 刀 R5a) — strict
    // gate + the boolean-answer runtime-descriptor define. Length 3.
    row("Reflect", "defineProperty", 3),
    // §28.1.1 Reflect.apply (rotation 267 刀 R6) — IsCallable gate +
    // the Function.prototype.apply kernel. Length 3.
    row("Reflect", "apply", 3),
    // §28.1.13 Reflect.set (rotation 268) — strict gate + the
    // boolean-answer [[Set]] kernel. Length 3 per spec.
    row("Reflect", "set", 3),
    // proposal-array-from-async §2.1.1 Array.fromAsync (rotation 275
    // 刀 2) — a detached call has an undefined |this|, which is not a
    // constructor, so §3.k.iv falls to ArrayCreate: the arm delegates
    // to the same sync-source kernels the direct-call lowering bakes.
    // Length 1 per proposal (mapfn / thisArg optional).
    row("Array", "fromAsync", 1),
    // §27.2.4.8 Promise.try — step 1 requires the |this| value to be
    // an object (the species ctor); a detached call's undefined
    // |this| raises the same TypeError as resolve / reject
    // (Disp::PromiseSettle). The direct-call form desugars at the
    // AST layer and never reaches this cell. Length 1 per spec.
    row("Promise", "try", 1),
    // ES2026 json-parse-with-source JSON.rawJSON / isRawJSON
    // (rotation 275 刀 2) — same-crate kernels (json_raw.rs), so the
    // detached call face is the real §25.5.1/.3 semantics. Lengths
    // per spec.
    row("JSON", "rawJSON", 1),
    row("JSON", "isRawJSON", 1),
    // §25.5.1/.2 JSON.parse / JSON.stringify — same-crate any-lane
    // kernels (json_any.rs / json_reviver.rs / json_stringify.rs),
    // so the detached call face is the real semantics. The JSON
    // namespace-object singleton mint fills these cells (RFC
    // 20260801-ns-object-value, JSON extension). Lengths per spec.
    row("JSON", "parse", 2),
    row("JSON", "stringify", 3),
    // §28.1.{6,9,11,2} Reflect get / has / ownKeys / construct —
    // strict IsObject gate + the existing any-lane kernels
    // (member-get / in-op / own-keys Names walk /
    // __torajs_reflect_construct). The Reflect namespace-object
    // singleton mint fills these cells alongside the nine rows
    // above. Lengths per spec.
    row("Reflect", "get", 2),
    row("Reflect", "has", 2),
    row("Reflect", "ownKeys", 1),
    row("Reflect", "construct", 2),
    // §19.2.1 the global `eval` as a VALUE — identity / typeof /
    // name / length are real; tr performs no runtime evaluation
    // (direct calls compile through the desugar_eval prefix), so a
    // call through the escaped cell raises the recorded loud
    // TypeError. Keyed under `globalThis` (its owning object).
    // Length 1 per spec.
    row("globalThis", "eval", 1),
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

/// Delete tombstones for table statics (`delete Promise.all` — the
/// spec {configurable: true} descriptor's delete leg): one bit per
/// id, mirror of `builtin_proto`'s DELETED_MIDS posture. Readers
/// probe the ctor cell's expando BEFORE the table, so a
/// defineProperty restore shadows the tombstone with no bit clear.
/// Atomic per the multi-thread-ready substrate rule.
static NS_STATIC_DELETED: [core::sync::atomic::AtomicU64; 4] = [
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
];

/// Record `delete <Ctor>.<static>` for a table id. Out-of-range ids
/// (table growth past 256 would be a build-time extension bug — see
/// the const assert below) are ignored.
pub fn ns_static_mark_deleted(id: i64) {
    if (0..(NS_STATIC_DELETED.len() as i64 * 64)).contains(&id) {
        NS_STATIC_DELETED[(id / 64) as usize]
            .fetch_or(1u64 << (id % 64), core::sync::atomic::Ordering::Relaxed);
    }
}

/// True when the table static has been tombstoned by a delete.
pub fn ns_static_is_deleted(id: i64) -> bool {
    (0..(NS_STATIC_DELETED.len() as i64 * 64)).contains(&id)
        && NS_STATIC_DELETED[(id / 64) as usize].load(core::sync::atomic::Ordering::Relaxed)
            & (1u64 << (id % 64))
            != 0
}

// Bitmask capacity must cover every table row.
const _: () = assert!(NS_STATIC_TABLE.len() <= 256);

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
