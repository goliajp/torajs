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

use core::ffi::c_void;

unsafe extern "C" {
    pub(super) fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — 1 when a pending throw is recorded (a poisoned
    /// valueOf during ToNumber aborts the remaining coercions).
    pub(super) fn __torajs_throw_check() -> i64;
    fn __torajs_math_sqrt(x: f64) -> f64;
    fn __torajs_math_abs(x: f64) -> f64;
    fn __torajs_math_floor(x: f64) -> f64;
    fn __torajs_math_ceil(x: f64) -> f64;
    fn __torajs_math_log(x: f64) -> f64;
    fn __torajs_math_exp(x: f64) -> f64;
    fn __torajs_math_sign(x: f64) -> f64;
    fn __torajs_math_round(x: f64) -> f64;
    fn __torajs_math_trunc(x: f64) -> f64;
    fn __torajs_math_sin(x: f64) -> f64;
    fn __torajs_math_cos(x: f64) -> f64;
    fn __torajs_math_tan(x: f64) -> f64;
    fn __torajs_math_asin(x: f64) -> f64;
    fn __torajs_math_acos(x: f64) -> f64;
    fn __torajs_math_atan(x: f64) -> f64;
    fn __torajs_math_log2(x: f64) -> f64;
    fn __torajs_math_log10(x: f64) -> f64;
    fn __torajs_math_cbrt(x: f64) -> f64;
    fn __torajs_math_sinh(x: f64) -> f64;
    fn __torajs_math_cosh(x: f64) -> f64;
    fn __torajs_math_tanh(x: f64) -> f64;
    fn __torajs_math_asinh(x: f64) -> f64;
    fn __torajs_math_acosh(x: f64) -> f64;
    fn __torajs_math_atanh(x: f64) -> f64;
    fn __torajs_math_expm1(x: f64) -> f64;
    fn __torajs_math_log1p(x: f64) -> f64;
    fn __torajs_math_fround(x: f64) -> f64;
    fn __torajs_math_f16round(x: f64) -> f64;
    fn __torajs_math_pow(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_min(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_max(x: f64, y: f64) -> f64;
    fn __torajs_math_atan2(y: f64, x: f64) -> f64;
    fn __torajs_math_imul(a: i64, b: i64) -> i64;
    fn __torajs_math_clz32(x: i64) -> i64;
    fn __torajs_math_random() -> f64;
    /// torajs-num — the typed tier's §19.2.5/.4 parse kernels
    /// (Str cell in, auto-detect radix on 0).
    pub(super) fn __torajs_num_parse_int(s: *const u8, radix: i64) -> f64;
    pub(super) fn __torajs_num_parse_float(s: *const u8) -> f64;
    /// torajs-str — release the owned coercion temp.
    pub(super) fn __torajs_str_drop(s: *mut c_void);
    /// torajs-meta — §20.1.2.17/.23/.5 own-enumeration. Each answers
    /// a FRESH Arr cell (rc 1); `include_nonenum` picks the
    /// `getOwnPropertyNames` surface over the `keys` one.
    ///
    /// `own_keys` is the one of the three that hands back an
    /// UNSTAMPED block: `own_values` allocates a real `Array<Any>`
    /// (`FLAG_ARR_ANY`) and `own_entries` stamps its outer array
    /// itself, but the keys walk just pushes raw Str pointers. The
    /// typed tier never noticed — its static `Arr<Str>` result type
    /// drives the element drops — so the arm has to stamp the kind
    /// at the typed→Any boundary or every heap key leaks.
    pub(super) fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    /// torajs-arr — stamp the element-kind field on a typed array
    /// crossing into the any world (the `exec` / `split` precedent).
    pub(super) fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    pub(super) fn __torajs_anyv_own_values(v: u64) -> *mut c_void;
    pub(super) fn __torajs_anyv_own_entries(v: u64) -> *mut c_void;
    /// torajs-meta — §20.1.2.1 single-source copy. Guards a
    /// null/undefined TARGET itself (the arm leans on that instead
    /// of re-deriving step 1); a null/undefined SOURCE is a no-op.
    pub(super) fn __torajs_anyv_assign(target: u64, source: u64);
    /// torajs-meta — §20.1.2.6. Returns the receiver bit pattern
    /// UNCHANGED and does NOT rc_inc: a borrow, so the arm owns it
    /// before handing it back.
    pub(super) fn __torajs_anyv_freeze(obj_any: u64) -> u64;
    /// torajs-rc — §20.1.2.13 NaN-box-aware probe (non-object reads
    /// `true` by definition). Answers a plain bool: no ownership.
    pub(super) fn __torajs_obj_is_frozen_any(v: i64) -> bool;
    /// torajs-meta — §20.1.2.12. Already OWNED on return (the
    /// builtin-prototype singletons and the dynobj slot read both
    /// rc_inc before answering), so the arm must NOT inc again.
    pub(super) fn __torajs_anyv_get_proto_of_any(v: u64) -> u64;
    /// torajs-meta — §20.1.2.21. Void; the static answers its
    /// receiver, so the arm owns the borrow before handing it back.
    pub(super) fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64);
    /// torajs-meta — §20.1.2.7. Answers a FRESH dynobj (owned); the
    /// reject paths answer an immediate, so nothing leaks there.
    pub(super) fn __torajs_anyv_from_entries(entries: u64) -> u64;
    /// torajs-str — §20.4.2.2 registry lookup-or-create. SHARES the
    /// key on a miss (`symbol_alloc` incs the desc itself), so the
    /// arm still drops its minted coercion temp; the returned Symbol
    /// is rc'd for the caller (owned).
    pub(super) fn __torajs_symbol_for(key: *mut c_void) -> *mut c_void;
    /// torajs-str — §20.4.2.6. Answers the registered key Str (rc'd,
    /// owned) or NULL for an unregistered symbol — NULL maps to
    /// undefined, never to a raw null Str slot.
    pub(super) fn __torajs_symbol_key_for(sym: *mut c_void) -> *mut c_void;
    /// torajs-date — §21.4.3.1 Date.now (ms since epoch, no alloc).
    pub(super) fn __torajs_date_now_static() -> i64;
    /// torajs-date — §21.4.3.2 Date.parse (ISO 8601 → ms; NaN on
    /// parse failure). `s` is a live Str cell.
    pub(super) fn __torajs_date_parse_iso(s: *const c_void) -> f64;
    /// torajs-date — §21.4.3.4 Date.UTC MakeTime over 7 components
    /// (TimeClip'd; NaN when any component is NaN / out of range).
    pub(super) fn __torajs_date_utc_components(
        year: f64,
        month: f64,
        day: f64,
        hour: f64,
        minute: f64,
        second: f64,
        milli: f64,
    ) -> f64;
    /// torajs-str — §22.1.2.1 one-code-unit mint (truncates
    /// `n & 0xFFFF` itself, never throws).
    pub(super) fn __torajs_str_from_char_code(n: i64) -> *mut u8;
    /// torajs-str — §22.1.2.2 one-code-point mint; out-of-range
    /// records a catchable RangeError and answers an empty sentinel.
    pub(super) fn __torajs_str_from_code_point(n: i64) -> *mut u8;
    /// torajs-meta — §20.1.2.16/.13/.20/.15 integrity family (RFC
    /// C5b). The setters answer the receiver as a BORROW (the arm
    /// owns it before handing it back); the readers answer plain
    /// bools.
    pub(super) fn __torajs_anyv_prevent_extensions(obj_any: u64) -> u64;
    pub(super) fn __torajs_anyv_is_extensible(obj_any: u64) -> bool;
    pub(super) fn __torajs_anyv_seal(obj_any: u64) -> u64;
    pub(super) fn __torajs_anyv_is_sealed(obj_any: u64) -> bool;
    /// torajs-throw — catchable RangeError for the §7.1.22 ToIndex
    /// rejects in the BigInt.asN arm.
    pub(super) fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    /// torajs-bigint — §21.2.2.{1,2} fixed-width views (arbitrary
    /// bits, 刀 5a). Fresh owned BigInt out; bits < 0 and the
    /// asUintN negative-input size cap record a RangeError and
    /// answer a `0n` sentinel.
    pub(super) fn __torajs_bigint_as_int_n(bits: i64, value: *const c_void) -> *mut u8;
    pub(super) fn __torajs_bigint_as_uint_n(bits: i64, value: *const c_void) -> *mut u8;
    /// torajs-bigint — release an owned BigInt stake (coercion temp
    /// / kernel result on the throw-unwind path).
    pub(super) fn __torajs_bigint_drop_rc(p: *mut c_void);
    /// torajs-meta — §20.1.2.10 W-N-c truth: tr has no symbol-keyed
    /// props, so the kernel answers a FRESH empty `Arr<Str>` (owned)
    /// for every object; a nullish receiver records its ToObject
    /// TypeError and still answers the well-formed empty Arr.
    pub(super) fn __torajs_anyv_own_symbols(obj_any: u64) -> *mut c_void;
}

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
    /// ToInt32 pair → i32 result (imul).
    I32Pair(unsafe extern "C" fn(i64, i64) -> i64),
    /// ToUint32 unary → i32-ranged result (clz32).
    I32One(unsafe extern "C" fn(i64) -> i64),
    /// () → f64 (random).
    Nullary(unsafe extern "C" fn() -> f64),
    /// WHATWG console stdout logger — per-arg tag-aware inline
    /// print + `' '` separators + `'\n'` (the chunk-808 multiarg
    /// phase-2 sequence; args are already evaluated in argv).
    ConsoleLog,
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
    Disp::ConsoleLog, // console.log
    Disp::ConsoleLog, // console.info — same stream per §1.1.2/.4
    Disp::ConsoleLog, // console.debug
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
];
