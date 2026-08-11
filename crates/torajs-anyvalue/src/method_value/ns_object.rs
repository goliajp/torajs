//! RFC 20260801-ns-object-value — the `Math` NAMESPACE OBJECT as a
//! first-class value.
//!
//! `Math` read in a value position (thisArg / call arg / return /
//! `this === Math` identity) answers ONE immortal dynobj singleton,
//! lazily minted and pre-filled with every Math static the
//! ns-static value face already interns (RFC 20260719 — the same
//! cells `Math.max`-as-a-value answers, so a post-escape dynamic
//! call rides the existing boxed dual entry) plus the eight
//! §21.3.1 value properties. Being a real dynobj, member reads,
//! expando writes, delete, freeze and for-in all take the ordinary
//! dynobj lanes with zero new dispatch surface.
//!
//! Those entries are DEFINED, not written. A built-in's own property
//! is not an ordinary write: §17 makes every function property
//! non-enumerable, and §21.3.1 makes the eight constants non-writable,
//! non-enumerable and non-configurable. Filling with plain writes gave
//! all 43 entries a write's defaults, and the object then answered a
//! different fact to each surface that asked — `Object.keys(Math)` said
//! 43, `getOwnPropertyDescriptor` said `configurable: true`, and
//! `delete Math.PI` removed the dict entry and answered true while
//! `Math.PI` kept reading its compile-time constant.
//!
//! `Object.prototype.toString.call(Math)` answers "[object Math]"
//! per §21.3.1.9 through the pointer-identity probe below (the
//! badge walk in `method_call_object_proto::cell_badge` checks it
//! before the generic dynobj fallback).

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::ns_static::NS_STATIC_TABLE;
use torajs_rc::{AnySlotTag, FLAG_STATIC_LITERAL};

use crate::AnyValue;
use crate::nanbox::box_void_ptr;

use super::mint_immortal_str;
use super::ns_static::ns_static_cell;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-dynobj — define kernel (§10.1.6.3 apply core). The
    /// namespace fills through this rather than `__torajs_dynobj_set`
    /// because a built-in's own properties are not ordinary writes:
    /// see the flag mirrors below.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
}

/// Flag-byte mirror of `torajs_dynobj::layout::DEFINE_*` (same shape
/// the `object_proto_install` / legacy-accessor mirrors take).
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;

/// §21.3.1 — the eight value properties are
/// `{ [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]:
/// false }`. Every attribute is stated (PRESENT with a 0 value) so the
/// entry says so rather than inheriting a write's defaults.
const MATH_CONST_FLAGS: u64 = DEFINE_PRESENT_WRITABLE
    | DEFINE_PRESENT_ENUMERABLE
    | DEFINE_PRESENT_CONFIGURABLE
    | DEFINE_PRESENT_VALUE;

/// §17 — a built-in function property is `{ [[Writable]]: true,
/// [[Enumerable]]: false, [[Configurable]]: true }`. Only
/// `enumerable` separates it from an ordinary write, and that one
/// difference is what keeps `Math` out of `Object.keys` / for-in /
/// JSON / spread.
const MATH_METHOD_FLAGS: u64 = DEFINE_FLAG_WRITABLE
    | DEFINE_FLAG_CONFIGURABLE
    | DEFINE_PRESENT_WRITABLE
    | DEFINE_PRESENT_ENUMERABLE
    | DEFINE_PRESENT_CONFIGURABLE
    | DEFINE_PRESENT_VALUE;

/// The interned Math singleton — 0 until first minted.
static MATH_OBJECT: AtomicU64 = AtomicU64::new(0);

/// §21.3.1 value properties (spec order).
const MATH_CONSTS: &[(&[u8], f64)] = &[
    (b"E", core::f64::consts::E),
    (b"LN10", core::f64::consts::LN_10),
    (b"LN2", core::f64::consts::LN_2),
    (b"LOG10E", core::f64::consts::LOG10_E),
    (b"LOG2E", core::f64::consts::LOG2_E),
    (b"PI", core::f64::consts::PI),
    (b"SQRT1_2", core::f64::consts::FRAC_1_SQRT_2),
    (b"SQRT2", core::f64::consts::SQRT_2),
];

/// §17 function-property fill — every ns-static row of `ns` DEFINED
/// on `obj` (the interned cells are the same ones the member value
/// read answers, so identity holds across both surfaces).
///
/// # Safety
/// `obj` points at a live dynobj slot.
unsafe fn fill_ns_methods(obj: &mut *mut c_void, ns: &str) {
    unsafe {
        for (id, row) in NS_STATIC_TABLE.iter().enumerate() {
            if row.ns != ns {
                continue;
            }
            let cell = ns_static_cell(id as i64);
            let key = mint_immortal_str(row.name.as_bytes());
            __torajs_dynobj_define(
                obj,
                key as *mut c_void,
                AnySlotTag::Heap as u64,
                cell as u64,
                MATH_METHOD_FLAGS,
            );
        }
    }
}

/// Intern `mint()`'s object into `slot` — the compare-exchange keeps
/// identity unique under concurrent first touches (multi-thread-ready
/// shape; the loser leaks one pre-filled object, which the immortal
/// flag makes inert).
fn intern_singleton(slot: &AtomicU64, mint: impl FnOnce() -> *mut c_void) -> *mut c_void {
    let p = slot.load(Ordering::Acquire);
    if p != 0 {
        return p as *mut c_void;
    }
    let obj = mint();
    // SAFETY: `mint` filled the fresh dynobj with immortal cells;
    // the static flag makes rc traffic no-op so scope drops never
    // reclaim it.
    unsafe {
        *(obj.cast::<u8>().add(6) as *mut u16) |= FLAG_STATIC_LITERAL;
    }
    match slot.compare_exchange(0, obj as u64, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => obj,
        Err(winner) => winner as *mut c_void,
    }
}

/// The Math singleton's dynobj pointer — mints on first use.
pub(crate) fn math_object_ptr() -> *mut c_void {
    intern_singleton(&MATH_OBJECT, || unsafe {
        let mut obj = __torajs_dynobj_alloc();
        fill_ns_methods(&mut obj, "Math");
        for (name, v) in MATH_CONSTS {
            let key = mint_immortal_str(name);
            __torajs_dynobj_define(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::F64 as u64,
                v.to_bits(),
                MATH_CONST_FLAGS,
            );
        }
        obj
    })
}

/// The interned JSON singleton — 0 until first minted.
static JSON_OBJECT: AtomicU64 = AtomicU64::new(0);

/// The JSON singleton's dynobj pointer (§25.5) — parse / stringify /
/// rawJSON / isRawJSON function properties, no value properties.
pub(crate) fn json_object_ptr() -> *mut c_void {
    intern_singleton(&JSON_OBJECT, || unsafe {
        let mut obj = __torajs_dynobj_alloc();
        fill_ns_methods(&mut obj, "JSON");
        obj
    })
}

/// The interned console singleton — 0 until first minted.
static CONSOLE_OBJECT: AtomicU64 = AtomicU64::new(0);

/// The console singleton's dynobj pointer (WHATWG console §1.1) —
/// the five logger function properties the ns-static table carries
/// (log / info / debug / error / warn), no value properties. RFC
/// 20260812-console-sink knife 4.
pub(crate) fn console_object_ptr() -> *mut c_void {
    intern_singleton(&CONSOLE_OBJECT, || unsafe {
        let mut obj = __torajs_dynobj_alloc();
        fill_ns_methods(&mut obj, "console");
        obj
    })
}

/// The interned Reflect singleton — 0 until first minted.
static REFLECT_OBJECT: AtomicU64 = AtomicU64::new(0);

/// The Reflect singleton's dynobj pointer (§28.1) — the thirteen
/// function properties the ns-static table carries, no value
/// properties.
pub(crate) fn reflect_object_ptr() -> *mut c_void {
    intern_singleton(&REFLECT_OBJECT, || unsafe {
        let mut obj = __torajs_dynobj_alloc();
        fill_ns_methods(&mut obj, "Reflect");
        obj
    })
}

/// Compiler face — `Math` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_object_math() -> AnyValue {
    box_void_ptr(math_object_ptr())
}

/// Compiler face — `JSON` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_object_json() -> AnyValue {
    box_void_ptr(json_object_ptr())
}

/// Compiler face — `Reflect` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_object_reflect() -> AnyValue {
    box_void_ptr(reflect_object_ptr())
}

/// Compiler face — `console` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_object_console() -> AnyValue {
    box_void_ptr(console_object_ptr())
}

/// Compiler face — the global `eval` lowered in a value position
/// (§19.2.1). The interned ns-static cell carries the name / length
/// reflection and the recorded loud call face; identity holds
/// across every read because the cell interns per id.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_global_eval_value() -> AnyValue {
    let id = torajs_rc::ns_static::ns_static_id("globalThis", "eval");
    box_void_ptr(ns_static_cell(id).cast())
}

/// Identity probe for the toString badge — true only for the minted
/// singleton (0-compare before any mint, so a plain dynobj never
/// false-positives).
pub(crate) fn is_math_object(ptr: *const c_void) -> bool {
    let p = MATH_OBJECT.load(Ordering::Relaxed);
    p != 0 && p == ptr as u64
}

/// [`is_math_object`]'s JSON twin — the §25.5.3 @@toStringTag badge.
pub(crate) fn is_json_object(ptr: *const c_void) -> bool {
    let p = JSON_OBJECT.load(Ordering::Relaxed);
    p != 0 && p == ptr as u64
}

/// [`is_math_object`]'s Reflect twin — the §28.1.14 @@toStringTag
/// badge.
pub(crate) fn is_reflect_object(ptr: *const c_void) -> bool {
    let p = REFLECT_OBJECT.load(Ordering::Relaxed);
    p != 0 && p == ptr as u64
}

/// [`is_math_object`]'s console twin — the Web IDL namespace-object
/// @@toStringTag badge (`[object console]`, bun-verified).
pub(crate) fn is_console_object(ptr: *const c_void) -> bool {
    let p = CONSOLE_OBJECT.load(Ordering::Relaxed);
    p != 0 && p == ptr as u64
}
