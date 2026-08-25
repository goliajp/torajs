//! RFC 20260807-global-object G2 — `globalThis` as a first-class
//! VALUE: one immortal dynobj singleton (the `Math` ns-object lane,
//! RFC 20260801).
//!
//! The fill list is everything the runtime can already mint an
//! identity for: the §19.1.1-3 value properties (`globalThis`
//! self-reference, `Infinity`, `NaN`, `undefined`), the `Math`
//! namespace singleton, and the interned builtin-constructor cells
//! (`Object` / `Array` / … — the same identities the bare ident read
//! answers, so `globalThis["Array"] === Array` holds through the
//! dynamic lane too).
//!
//! Entries are DEFINED, not written (§17 / §19.1: function and
//! constructor properties are writable+configurable but
//! non-enumerable; the three value constants are non-everything;
//! `globalThis` itself is writable+configurable, non-enumerable).
//!
//! The §20.5 Error family fills from the torajs-rc
//! `native_error_class` registry (recorded at module init by the
//! injected classes' `error_proto_install` emits), so the dynamic
//! read answers the bare name's class object.
//!
//! A KNOWN builtin global the runtime cannot mint THIS program
//! (`fetch`, a NativeError class the program never injected, …) is
//! NOT filled — a dynamic read must stay LOUD, not answer a silent
//! `undefined` where bun answers a function. The member-get miss
//! lane consults [`globalthis_missing_known`] via the singleton
//! pointer probe and raises a TypeError. Static spellings
//! (`globalThis.parseInt`) never get here — the G1 desugar rewrote
//! them to the bare name at compile time. An UNKNOWN name answers
//! `undefined` like every ordinary miss (bun parity).
//!
//! Mutations never reach this object: the checker rejects member
//! stores / deletes / incrs through a bare `globalThis` (a write
//! landing here would diverge from the bare name's static
//! resolution — the exact silent wrong G2 refuses).

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::builtin_proto::native_error_class;
use torajs_rc::{AnySlotTag, FLAG_STATIC_LITERAL};

use crate::AnyValue;
use crate::nanbox::box_void_ptr;

use super::ctor::builtin_ctor_cell;
use super::mint_immortal_str;
use super::ns_object::{console_object_ptr, json_object_ptr, math_object_ptr, reflect_object_ptr};
use crate::dispatch_seam::__torajs_ns_static_cell as ns_static_cell;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
}

const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;

/// §19.1.1-3 — `Infinity` / `NaN` / `undefined` are `{ [[Writable]]:
/// false, [[Enumerable]]: false, [[Configurable]]: false }`.
const VALUE_PROP_FLAGS: u64 = DEFINE_PRESENT_WRITABLE
    | DEFINE_PRESENT_ENUMERABLE
    | DEFINE_PRESENT_CONFIGURABLE
    | DEFINE_PRESENT_VALUE;

/// §17 / §19.1.1.1 — constructor properties and the `globalThis`
/// self-reference are `{ [[Writable]]: true, [[Enumerable]]: false,
/// [[Configurable]]: true }`.
const REF_PROP_FLAGS: u64 = DEFINE_FLAG_WRITABLE
    | DEFINE_FLAG_CONFIGURABLE
    | DEFINE_PRESENT_WRITABLE
    | DEFINE_PRESENT_ENUMERABLE
    | DEFINE_PRESENT_CONFIGURABLE
    | DEFINE_PRESENT_VALUE;

/// The interned globalThis singleton — 0 until first minted.
static GLOBALTHIS_OBJECT: AtomicU64 = AtomicU64::new(0);

/// The builtin constructors whose interned ctor cell exists —
/// `(name, proto tag)`, tags per the compiler's `builtin_proto_tag`
/// table (torajs-core `ssa_lower_member_builtin_namespace.rs`).
const CTOR_PROPS: &[(&[u8], i64)] = &[
    (b"Number", 0),
    (b"Object", 1),
    (b"Array", 2),
    (b"String", 3),
    (b"Boolean", 4),
    (b"Symbol", 5),
    (b"BigInt", 6),
    (b"RegExp", 7),
    (b"Date", 8),
    (b"Promise", 10),
    (b"Map", 11),
    (b"Set", 12),
    (b"Function", 13),
    (b"Iterator", 15),
    (b"WeakMap", 16),
    (b"WeakSet", 17),
    (b"WeakRef", 18),
];

/// Known builtin globals the fill list CANNOT always mint — a
/// dynamic read that misses must throw rather than answer a silent
/// `undefined`. Mirror of `is_known_builtin_global` (torajs-core
/// `check_js_semantics.rs`) minus the unconditionally filled names.
/// The NativeError rows stay listed even though the Error-family
/// fill usually defines them: a program that never injects one (a
/// computed-string read is invisible to the injection's reference
/// scan) leaves the fill empty, and the miss keeps the loud posture.
const MISSING_KNOWN: &[&[u8]] = &[
    b"TypeError",
    b"RangeError",
    b"ReferenceError",
    b"SyntaxError",
    b"EvalError",
    b"URIError",
    b"Bun",
    b"fetch",
    b"process",
    b"queueMicrotask",
];

/// The globalThis singleton's dynobj pointer — mints on first use
/// (compare-exchange identity, multi-thread-ready shape; a losing
/// racer leaks one inert pre-filled object).
pub(crate) fn globalthis_object_ptr() -> *mut c_void {
    let p = GLOBALTHIS_OBJECT.load(Ordering::Acquire);
    if p != 0 {
        return p as *mut c_void;
    }
    // SAFETY: fresh dynobj filled with immortal cells; the object
    // takes the static flag so rc traffic no-ops. The `globalThis`
    // self-reference defines LAST: `__torajs_dynobj_define` may move
    // the object on growth (it updates `obj` through the slot), and
    // a self-pointer stored before a move would go stale.
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        for (name, tag) in CTOR_PROPS {
            let key = mint_immortal_str(name);
            __torajs_dynobj_define_plain(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::Heap as u64,
                builtin_ctor_cell(*tag) as u64,
                REF_PROP_FLAGS,
            );
        }
        // §20.5 Error family — the injected class objects, recorded
        // at module init (torajs-meta `error_proto_install` → the
        // torajs-rc registry). Defining the SAME object the bare name
        // resolves to keeps identity (`globalThis.TypeError ===
        // TypeError`); a member this program never injected stays out,
        // so its dynamic read keeps the MISSING_KNOWN loud TypeError.
        // `Error` alone falls back to the interned ctor cell so the
        // row never vanishes (the pre-registry fill shape).
        for (i, fam) in native_error_class::NATIVE_ERROR_FAMILY.iter().enumerate() {
            let mut v = native_error_class::native_error_class(i);
            if v == 0 {
                if *fam != "Error" {
                    continue;
                }
                v = builtin_ctor_cell(9) as u64;
            } else {
                // The define transfers a value stake; class objects
                // are ordinary rc cells (the static ctor-cell
                // fallback branch no-ops rc traffic).
                torajs_rc::__torajs_rc_inc(v as *mut c_void);
            }
            let key = mint_immortal_str(fam.as_bytes());
            __torajs_dynobj_define_plain(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::Heap as u64,
                v,
                REF_PROP_FLAGS,
            );
        }
        let key = mint_immortal_str(b"Math");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            math_object_ptr() as u64,
            REF_PROP_FLAGS,
        );
        let key = mint_immortal_str(b"JSON");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            json_object_ptr() as u64,
            REF_PROP_FLAGS,
        );
        let key = mint_immortal_str(b"Reflect");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            reflect_object_ptr() as u64,
            REF_PROP_FLAGS,
        );
        // WHATWG console §1.1 (RFC 20260812-console-sink knife 4) —
        // the same interned singleton the bare `console` value read
        // answers, so `globalThis.console === console` holds.
        let key = mint_immortal_str(b"console");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            console_object_ptr() as u64,
            REF_PROP_FLAGS,
        );
        // §19.2.1 — the same interned cell the bare `eval` ident
        // answers, so identity holds through the dynamic lane too.
        let key = mint_immortal_str(b"eval");
        let eval_cell = ns_static_cell(torajs_rc::ns_static::ns_static_id("globalThis", "eval"));
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            eval_cell as u64,
            REF_PROP_FLAGS,
        );
        // §19.2.4 / §19.2.5 — the global `parseFloat` / `parseInt`
        // ARE the `Number.parseFloat` / `Number.parseInt` function
        // objects (§21.1.2.12/.13 "the same built-in function
        // object"), so the fill reuses those interned cells and
        // `globalThis.parseInt === Number.parseInt` holds like bun.
        // §19.2.2 / §19.2.3 — `isFinite` / `isNaN` are their own
        // ToNumber-coercing cells (globalThis-keyed rows), and the
        // §19.2.6 URI quartet rides the same lane.
        for (ns, name) in [
            ("Number", &b"parseInt"[..]),
            ("Number", b"parseFloat"),
            ("globalThis", b"isFinite"),
            ("globalThis", b"isNaN"),
            ("globalThis", b"decodeURI"),
            ("globalThis", b"decodeURIComponent"),
            ("globalThis", b"encodeURI"),
            ("globalThis", b"encodeURIComponent"),
        ] {
            let key = mint_immortal_str(name);
            let cell = ns_static_cell(torajs_rc::ns_static::ns_static_id(
                ns,
                core::str::from_utf8(name).expect("ascii name"),
            ));
            __torajs_dynobj_define_plain(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::Heap as u64,
                cell as u64,
                REF_PROP_FLAGS,
            );
        }
        let key = mint_immortal_str(b"Infinity");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::F64 as u64,
            f64::INFINITY.to_bits(),
            VALUE_PROP_FLAGS,
        );
        let key = mint_immortal_str(b"NaN");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::F64 as u64,
            f64::NAN.to_bits(),
            VALUE_PROP_FLAGS,
        );
        let key = mint_immortal_str(b"undefined");
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Undef as u64,
            0,
            VALUE_PROP_FLAGS,
        );
        let key = mint_immortal_str(b"globalThis");
        let self_ptr = obj;
        __torajs_dynobj_define_plain(
            &mut obj,
            key as *mut c_void,
            AnySlotTag::Heap as u64,
            self_ptr as u64,
            REF_PROP_FLAGS,
        );
        debug_assert!(core::ptr::eq(obj, self_ptr));
        *(obj.cast::<u8>().add(6) as *mut u16) |= FLAG_STATIC_LITERAL;
        match GLOBALTHIS_OBJECT.compare_exchange(0, obj as u64, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => obj,
            Err(winner) => winner as *mut c_void,
        }
    }
}

/// Compiler face — `globalThis` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_globalthis_object() -> AnyValue {
    box_void_ptr(globalthis_object_ptr())
}

/// Miss-lane probe: true only when `ptr` is the minted singleton AND
/// `key` spells a known builtin the fill list is missing — the
/// member-get miss raises a TypeError instead of a silent
/// `undefined`. Unknown names keep the ordinary miss (undefined).
pub(crate) fn globalthis_missing_known(ptr: *const c_void, key: *const c_void) -> bool {
    let p = GLOBALTHIS_OBJECT.load(Ordering::Relaxed);
    if p == 0 || p != ptr as u64 {
        return false;
    }
    MISSING_KNOWN
        .iter()
        .any(|name| unsafe { crate::prop_has::key_is(key, name) })
}
