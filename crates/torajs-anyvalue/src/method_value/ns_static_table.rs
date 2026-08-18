//! Namespace-static dispatch table + kernel externs (RFC
//! 20260719-ns-static-value-reify) — split from
//! `method_value/ns_static.rs` under the 500-line file rule. Holds
//! the `Disp` shape enum, the id-indexed `DISPATCH` table
//! (index-lockstep with `torajs_rc::ns_static::NS_STATIC_TABLE`,
//! asserted in the parent's unit tests), and the cross-staticlib
//! kernel declarations the arms delegate to.
//!
//! MAINTENANCE: every extern added here needs a matching no-op stub
//! in `lib.rs`'s `#[cfg(test)] mod tests` — the table is
//! test-reachable, so `-dead_strip` keeps this module and the test
//! binary link fails on any unstubbed symbol (bitten twice: the
//! inspect print chain, then the num parse pair).

pub(super) use super::ns_static_externs::*;

/// Per-id dispatch shape. Index-lockstep with
/// [`torajs_rc::ns_static::NS_STATIC_TABLE`].
pub(super) enum Disp {
    /// f64 → f64 unary (argc 0 coerces undefined → NaN).
    F(unsafe extern "C" fn(f64) -> f64),
    /// f64 × f64 → f64 binary (missing args coerce to NaN).
    Ff(unsafe extern "C" fn(f64, f64) -> f64),
    /// §21.3.2.24/25 variadic reduction (empty → ±Infinity).
    MinMax {
        is_max: bool,
    },
    /// §21.3.2.18 variadic sum²+sqrt (empty → +0; any Infinity wins
    /// over NaN per steps 3-4, which a plain sum cannot express).
    Hypot,
    /// ToInt32 pair → i32 result (imul).
    I32Pair(unsafe extern "C" fn(i64, i64) -> i64),
    /// ToUint32 unary → i32-ranged result (clz32).
    I32One(unsafe extern "C" fn(i64) -> i64),
    /// () → f64 (random).
    Nullary(unsafe extern "C" fn() -> f64),
    /// WHATWG console logger — per-arg tag-aware inline print +
    /// `' '` separators + `'\n'` (the chunk-808 multiarg phase-2
    /// sequence; args are already evaluated in argv). `to_stderr`
    /// brackets the walk with torajs-io's current-sink switch
    /// (error / warn, RFC 20260812-console-sink knife 3).
    ConsoleLog {
        to_stderr: bool,
    },
    /// §19.2.5 parseInt — ToString(arg0) + ToInt32(radix) into the
    /// typed tier's parse kernel.
    ParseInt,
    /// §19.2.4 parseFloat — ToString(arg0) into the parse kernel.
    ParseFloat,
    /// §21.1.2 Number predicate family — computed inline on the
    /// NaN-box (spec: non-number input answers false, NO coercion).
    NumPredicate(NumPred),
    /// §23.1.2.2 Array.isArray — heap-tag probe.
    ArrayIsArray,
    /// §20.1.2.14 Object.is — the §7.2.10 same-value kernel.
    ObjectIs,
    /// §20.1.2.{17,23,5} own-enumeration — the kernel answers a
    /// fresh Arr, which IS the owned result (no inc).
    OwnEnum(OwnKind),
    /// §20.1.2.1 Object.assign — variadic fold over the sources,
    /// answering the target as an owned reference.
    ObjectAssign,
    /// §20.1.2.6 Object.freeze — kernel answers a borrow.
    ObjectFreeze,
    /// §20.1.2.13 Object.isFrozen — bool immediate.
    ObjectIsFrozen,
    /// §20.1.2.12 Object.getPrototypeOf — kernel answers an OWNED
    /// reference already.
    ObjectGetProtoOf,
    /// §20.1.2.21 Object.setPrototypeOf — void kernel; the static
    /// answers its receiver.
    ObjectSetProtoOf,
    /// §20.1.2.7 Object.fromEntries — kernel answers a fresh dynobj.
    ObjectFromEntries,
    /// §20.4.2.2 Symbol.for — ToString(key) into the registry kernel
    /// (owned Symbol out).
    SymbolFor,
    /// §20.4.2.6 Symbol.keyFor — Symbol-cell receiver check, then
    /// the registry scan (owned Str out, NULL → undefined).
    SymbolKeyFor,
    /// §21.4.3.1 Date.now — ms since epoch as a spec number.
    DateNow,
    /// §21.4.3.2 Date.parse — ToString(arg0) into the ISO kernel.
    DateParse,
    /// §21.4.3.4 Date.UTC — up to 7 ToNumber components in source
    /// order; absent trailing args take the spec defaults (year NaN,
    /// month 0, day 1, rest 0).
    DateUtc,
    /// §22.1.2.1/.2 String.fromCharCode / fromCodePoint — variadic
    /// per-code mint + pairwise concat (the typed lowering's chain).
    StrFromCodes {
        code_point: bool,
    },
    /// §20.1.2.11 Object.hasOwn — HasOwnProperty(ToObject(O), P).
    ObjectHasOwn,
    /// §20.1.2.16/.13 preventExtensions / isExtensible — header-flag
    /// setter (answers the receiver, owned by the arm) / reader.
    ObjectPreventExtensions,
    ObjectIsExtensible,
    /// §20.1.2.20/.15 seal / isSealed — header markers + the DynObj
    /// per-entry configurable walk / reader.
    ObjectSeal,
    ObjectIsSealed,
    /// §21.2.2.1/.2 BigInt.asIntN / asUintN — ToIndex(bits) +
    /// ToBigInt(value) into the fixed-width view kernels.
    BigIntAsN {
        signed: bool,
    },
    /// §27.2.4.7/.6 Promise.resolve / reject — a bare cell call has
    /// an undefined |this| and both statics require an object this
    /// (species ctor, step 1), so the arm always raises the bun/JSC
    /// TypeError; the cell exists for the reflection surface.
    PromiseSettle,
    /// §20.1.2.8 Object.getOwnPropertyDescriptor — ToString(P) into
    /// the meta descriptor kernel (fresh descriptor dynobj /
    /// undefined; kernel gates the nullish receiver).
    Gopd,
    /// §20.1.2.{9,2,4,3} getOwnPropertyDescriptors / create /
    /// defineProperty / defineProperties — reflection-surface cells
    /// (typeof / name / length / gOPD identity); the call face
    /// needs the any-tier define kernel (dynobj-slot writeback,
    /// RFC 20260721 records the face), so the arm raises a loud
    /// TypeError.
    DefineFace,
    /// §20.1.2.10 Object.getOwnPropertySymbols — the W-N-c
    /// empty-list kernel (same truth the typed call lane bakes).
    OwnSymbols,
    /// §23.1.2.1 Array.from — reflection-surface cell; the call
    /// face needs the iterator protocol + mapFn over any source
    /// (RFC 20260721 records the face), so the arm raises a loud
    /// TypeError.
    ArrayFromFace,
    /// §27.1.2.1 Iterator.from — the GetIteratorFlattenable kernel,
    /// AnyValue → owned AnyValue direct.
    IteratorFrom,
    /// proposal-iterator-sequencing Iterator.concat — argv packs
    /// into the fresh `Array<Any>` the kernel's items slot takes.
    IteratorConcat,
    /// proposal-joint-iteration Iterator.zip / zipKeyed —
    /// (iterables, options?) direct into the joint kernels.
    IteratorZip {
        keyed: bool,
    },
    /// §28.1.5 Reflect.getOwnPropertyDescriptor — strict IsObject
    /// gate (step 1 throws on every primitive, no ToObject) in front
    /// of the same descriptor path as `Disp::Gopd`.
    ReflectGopd,
    /// §28.1.4 Reflect.getPrototypeOf — strict gate + the proto
    /// classifier kernel (`Disp::ObjectGetProtoOf`'s path).
    ReflectGetProto,
    /// §28.1.10 Reflect.preventExtensions — strict gate + the
    /// header-flag setter; answers boolean true (ordinary
    /// [[PreventExtensions]] always succeeds), not the receiver.
    ReflectPreventExtensions,
    /// §28.1.8 Reflect.isExtensible — strict gate + the header-flag
    /// reader.
    ReflectIsExtensible,
    /// §28.1.3 Reflect.deleteProperty — strict gate + the
    /// OrdinaryDelete kernel (ToString(P) key; Bool answer).
    ReflectDeleteProperty,
    /// §28.1.12 Reflect.setPrototypeOf — strict gate + the
    /// boolean-answer OrdinarySetPrototypeOf core.
    ReflectSetPrototypeOf,
    /// §23.1.2.3 Array.of — argv packs into a fresh `Array<Any>`
    /// (the `Iterator.concat` pack shape, minus the kernel hop).
    ArrayOf,
    /// ES2025 §22.2.5.1 RegExp.escape — strict String gate + the
    /// torajs-regex EncodeForRegExpEscape kernel.
    RegExpEscape,
    /// §28.1.2 Reflect.defineProperty — strict gate + the
    /// boolean-answer runtime-descriptor define kernel (refusal =
    /// false, no throw; a ToPropertyDescriptor throw propagates).
    ReflectDefineProperty,
    /// §28.1.1 Reflect.apply — IsCallable gate + the
    /// Function.prototype.apply kernel (nullish argumentsList
    /// throws, no empty-list amnesty).
    ReflectApply,
    /// §28.1.13 Reflect.set — strict gate + the boolean-answer
    /// [[Set]] kernel (refusal = false, no throw; a setter throw
    /// propagates).
    ReflectSet,
    /// proposal-array-from-async §2.1.1 Array.fromAsync as a
    /// detached call — an undefined |this| is not a constructor, so
    /// §3.k.iv falls to ArrayCreate: the same sync-source kernels
    /// the direct-call lowering bakes (mapped form when a
    /// non-undefined mapfn argument is present).
    FromAsyncDyn,
    /// ES2026 json-parse-with-source §25.5.1/.3 — same-crate
    /// kernels (`crate::json_raw`), so the detached face IS the real
    /// semantics (rawJSON's TypeError / SyntaxError ride the pending
    /// throw out with the undefined answer).
    JsonRawJson,
    JsonIsRawJson,
    /// §25.5.1 JSON.parse — the same-crate any-lane parse kernel
    /// (`crate::json_any`), with the reviver walk when a second
    /// argument is present (`crate::json_reviver` gates
    /// IsCallable itself).
    JsonParse,
    /// §25.5.2 JSON.stringify — the same-crate any-lane walk
    /// (`crate::json_stringify`), space normalized through the gap
    /// kernel; `replacer` rides the same recorded ignore as the
    /// typed lowering (S311).
    JsonStringify,
    /// §28.1.6 Reflect.get — strict gate + the [[Get]] member-get
    /// kernel (2-arg form; a differing receiver argument is the
    /// recorded getter-this boundary).
    ReflectGet,
    /// §28.1.9 Reflect.has — strict gate + the `in`-operator kernel.
    ReflectHas,
    /// §28.1.11 Reflect.ownKeys — strict gate + the include-nonenum
    /// own-keys walk (`OwnKind::Names`'s path; tr has no
    /// symbol-keyed props, so the symbol tail is empty).
    ReflectOwnKeys,
    /// §28.1.2 Reflect.construct — the same-crate reflective
    /// construct kernel (`crate::reflect_construct`), newTarget
    /// defaulting to target on the 2-arg form.
    ReflectConstructDyn,
    /// §19.2.1 the global `eval` cell — the call face is the
    /// recorded loud TypeError (tr performs no runtime evaluation;
    /// the cell exists for identity / typeof / the reflection
    /// surfaces).
    EvalDyn,
    /// §19.2.2/.3 the global `isFinite` / `isNaN` — ToNumber
    /// coercion first (may throw on Symbol / BigInt), then the
    /// float test. The Number.* `NumPredicate` arms never coerce.
    GlobalNumTest {
        finite: bool,
    },
    /// §19.2.6 the four URI globals — ToString the argument (may
    /// throw), then the torajs-str Encode / Decode kernel
    /// (URIError on a malformed input).
    UriKernel {
        encode: bool,
        component: bool,
    },
    /// §20.1.2.10 Object.groupBy / §24.2.2.4 Map.groupBy — neither
    /// reads |this|, so the detached call IS the real semantics:
    /// (items, cb) straight into the torajs-meta kernels (fresh
    /// null-proto dynobj / fresh Map out, owned).
    GroupBy {
        map: bool,
    },
}

/// The own-enumeration surfaces (shared dispatch shape) — `Names`
/// is the §20.1.2.10 include-nonenum flavor of the keys walk.
pub(super) enum OwnKind {
    Keys,
    Values,
    Entries,
    Names,
}

/// The four `Number.is*` predicates (shared dispatch shape).
pub(super) enum NumPred {
    Integer,
    Nan,
    Finite,
    SafeInteger,
}

pub(super) static DISPATCH: &[Disp] = &[
    Disp::F(__torajs_math_sqrt),
    Disp::F(__torajs_math_abs),
    Disp::F(__torajs_math_floor),
    Disp::F(__torajs_math_ceil),
    Disp::F(__torajs_math_log),
    Disp::F(__torajs_math_exp),
    Disp::F(__torajs_math_sign),
    Disp::F(__torajs_math_round),
    Disp::F(__torajs_math_trunc),
    Disp::F(__torajs_math_sin),
    Disp::F(__torajs_math_cos),
    Disp::F(__torajs_math_tan),
    Disp::F(__torajs_math_asin),
    Disp::F(__torajs_math_acos),
    Disp::F(__torajs_math_atan),
    Disp::F(__torajs_math_log2),
    Disp::F(__torajs_math_log10),
    Disp::F(__torajs_math_cbrt),
    Disp::F(__torajs_math_sinh),
    Disp::F(__torajs_math_cosh),
    Disp::F(__torajs_math_tanh),
    Disp::F(__torajs_math_asinh),
    Disp::F(__torajs_math_acosh),
    Disp::F(__torajs_math_atanh),
    Disp::F(__torajs_math_expm1),
    Disp::F(__torajs_math_log1p),
    Disp::F(__torajs_math_fround),
    Disp::F(__torajs_math_f16round),
    Disp::Ff(__torajs_math_pow),
    Disp::MinMax { is_max: false },
    Disp::MinMax { is_max: true },
    Disp::Ff(__torajs_math_atan2),
    Disp::I32Pair(__torajs_math_imul),
    Disp::I32One(__torajs_math_clz32),
    Disp::Nullary(__torajs_math_random),
    Disp::ConsoleLog { to_stderr: false }, // console.log
    Disp::ConsoleLog { to_stderr: false }, // console.info — same stream per §1.1.2/.4
    Disp::ConsoleLog { to_stderr: false }, // console.debug
    Disp::ParseInt,
    Disp::ParseFloat,
    Disp::NumPredicate(NumPred::Integer),
    Disp::NumPredicate(NumPred::Nan),
    Disp::NumPredicate(NumPred::Finite),
    Disp::NumPredicate(NumPred::SafeInteger),
    Disp::ArrayIsArray,
    Disp::ObjectIs,
    Disp::OwnEnum(OwnKind::Keys),
    Disp::OwnEnum(OwnKind::Values),
    Disp::OwnEnum(OwnKind::Entries),
    Disp::ObjectAssign,
    Disp::ObjectFreeze,
    Disp::ObjectIsFrozen,
    Disp::ObjectGetProtoOf,
    Disp::ObjectSetProtoOf,
    Disp::ObjectFromEntries,
    Disp::SymbolFor,
    Disp::SymbolKeyFor,
    Disp::DateNow,
    Disp::DateParse,
    Disp::DateUtc,
    Disp::StrFromCodes { code_point: false },
    Disp::StrFromCodes { code_point: true },
    Disp::ObjectHasOwn,
    Disp::OwnEnum(OwnKind::Names),
    Disp::ObjectPreventExtensions,
    Disp::ObjectIsExtensible,
    Disp::ObjectSeal,
    Disp::ObjectIsSealed,
    Disp::BigIntAsN { signed: true },
    Disp::BigIntAsN { signed: false },
    Disp::PromiseSettle,
    Disp::PromiseSettle,
    Disp::Gopd,
    Disp::DefineFace, // getOwnPropertyDescriptors
    Disp::DefineFace, // create
    Disp::DefineFace, // defineProperty
    Disp::DefineFace, // defineProperties
    Disp::OwnSymbols,
    Disp::ArrayFromFace,
    Disp::PromiseSettle, // Promise.all
    Disp::PromiseSettle, // Promise.allSettled
    Disp::PromiseSettle, // Promise.any
    Disp::PromiseSettle, // Promise.race
    Disp::IteratorFrom,
    Disp::IteratorConcat,
    Disp::IteratorZip { keyed: false },
    Disp::IteratorZip { keyed: true },
    Disp::ReflectGopd,
    Disp::ReflectGetProto,
    Disp::ReflectPreventExtensions,
    Disp::ReflectIsExtensible,
    Disp::ReflectDeleteProperty,
    Disp::ReflectSetPrototypeOf,
    Disp::ArrayOf,
    Disp::RegExpEscape,
    Disp::ReflectDefineProperty,
    Disp::ReflectApply,
    Disp::ReflectSet,
    Disp::FromAsyncDyn,
    Disp::PromiseSettle,
    Disp::JsonRawJson,
    Disp::JsonIsRawJson,
    Disp::JsonParse,
    Disp::JsonStringify,
    Disp::ReflectGet,
    Disp::ReflectHas,
    Disp::ReflectOwnKeys,
    Disp::ReflectConstructDyn,
    Disp::EvalDyn,
    Disp::GlobalNumTest { finite: true },
    Disp::GlobalNumTest { finite: false },
    Disp::UriKernel {
        encode: false,
        component: false,
    },
    Disp::UriKernel {
        encode: false,
        component: true,
    },
    Disp::UriKernel {
        encode: true,
        component: false,
    },
    Disp::UriKernel {
        encode: true,
        component: true,
    },
    Disp::ConsoleLog { to_stderr: true }, // console.error
    Disp::ConsoleLog { to_stderr: true }, // console.warn
    Disp::Hypot,
    Disp::GroupBy { map: false },
    Disp::GroupBy { map: true },
    Disp::PromiseSettle, // Promise.withResolvers — §27.2.4.8 step 1 needs a ctor |this|
];
