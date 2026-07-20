//! Builtin method reification (chunk 711) — `s.toUpperCase` read as
//! a VALUE off an `any` builtin receiver answers a real function
//! object instead of undefined (the chunk 521 recorded boundary).
//!
//! ES semantics: extracting a builtin method yields the UNBOUND
//! `Function.prototype` member — `typeof f` is "function",
//! `f === s.toUpperCase` is true (same function object), a bare
//! `f()` runs with `this = undefined` and throws (every builtin
//! brand-checks its receiver), and `f.call(recv, …)` /
//! `f.apply(recv, list)` re-binds the receiver.
//!
//! The projection: one interned immortal cell per method id —
//!
//! - Layout is a capture-less closure env (universal header +
//!   fn_addr + drop_fn + props + boxed_entry + one capture slot
//!   holding the method id), so every existing callable probe
//!   (`typeof` → "function", `closure_boxed_entry`, HOF callbacks,
//!   expando reads/writes, strict-eq pointer identity) works
//!   unchanged.
//! - The header carries [`FLAG_STATIC_LITERAL`] — rc traffic
//!   no-ops, the cycle collector skips it, the cell never drops
//!   (CPython-immortal shape). The borrow-shaped member-get pair
//!   hands it out without any ledger.
//! - `boxed_entry` points at [`bare_entry`] — a bare call (direct,
//!   HOF callback, any-call) is the ES `this = undefined` TypeError.
//! - `fn_addr` points at [`native_entry`] — an any→typed fn-slot
//!   cast that direct-calls the native entry throws instead of
//!   jumping to 0 (recorded boundary: the typed-tier result value
//!   is garbage until the pending throw propagates).
//! - `f.call` / `f.apply` on the cell short-circuit in
//!   `method_call_closure` via [`builtin_method_mid`] and
//!   re-dispatch the ORIGINAL method id with the thisArg as the
//!   receiver.
//!
//! [`builtin_method_supported`] is the exact per-receiver-shape
//! support table (mirrors each `method_call_*` arm's id-switch) —
//! a wrong-arm read (`(42 as any).slice`) stays undefined like bun,
//! never an optimistic function that would TypeError on call.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{ANY_METHOD_NEXT, ANY_METHOD_TO_STRING, ANY_METHOD_UNKNOWN, any_method_id};
use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::method_support::{
    arr_supports, closure_supports, date_supports, map_supports, num_supports, regexp_supports,
    set_supports, str_supports, weakmap_supports, weakset_supports,
};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str,
};

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Cell layout offsets — mirror of torajs-core `ssa_lower.rs`
/// closure-env constants.
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
// trace_fn slot @ 40 stays 0 (alloc_zeroed) — these cells are
// FLAG_STATIC_LITERAL, the cycle collector never walks them.
const CLOSURE_CAP_BASE_OFF: usize = 48;
const CELL_SIZE: usize = 56;

// Constructor-value family (L3b ④ + rotation 131) — split to the
// `method_value/ctor.rs` submodule (file-size HARD RULE); the
// re-export keeps every `crate::method_value::` consumer face.
mod ctor;
pub(crate) use ctor::{builtin_ctor_cell, ctor_cell_for_recv};

// Namespace-static value family (RFC 20260719-ns-static-value-reify)
// — `Math.max` as a VALUE: interned dispatcher cells keyed by the
// shared torajs-rc ns-static table.
mod ns_static;
// Ctor-static arms (RFC 20260720-ctor-static-reflection 刀 1).
mod ns_static_ctor;
mod ns_static_table;

/// Interned name Str layout — mirror of torajs-str
/// `layout::{STR_LEN_OFF, STR_DATA_OFF}` + the `IS_LATIN1` flags
/// bit (`layout::STR_FLAG_IS_LATIN1`; method names are ASCII).
pub(crate) const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;
const STR_FLAG_IS_LATIN1: u16 = 0x0002;

/// Method-id intern table span (ids are append-only; headroom
/// beyond the current max keeps future ids table-hits).
pub(crate) const TABLE_SIZE: usize = 256;

/// Per-mid interned cells. Atomic-static, NOT `thread_local!` —
/// std's lazy TLS machinery is unavailable inside the baked
/// staticlib runtime (same constraint as torajs-cycle / torajs-weak,
/// which use the AtomicPtr-static pattern). Relaxed is exact today
/// (single-threaded runtime) and a benign double-alloc race later —
/// both winners are immortal.
static METHOD_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Per-mid interned `.name` Str cells (chunk 715) — same
/// immortal-static shape as [`METHOD_CELLS`].
static METHOD_NAME_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Boxed dual entry of every reified method cell — a bare call is
/// the ES `this = undefined` TypeError.
unsafe extern "C" fn bare_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Native entry — an any→typed fn-slot cast direct-calls this
/// instead of jumping to 0. Arguments are ignored (safe under the C
/// calling convention); the pending throw propagates at the callee
/// boundary.
pub(crate) unsafe extern "C" fn native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    0
}

/// The interned cell for a method id — lazily allocated, immortal.
pub(crate) fn builtin_method_cell(mid: i64) -> *mut u8 {
    let slot = &METHOD_CELLS[mid as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        // Universal header: refcount 1 (never reaches 0 — rc
        // traffic no-ops on the static flag), Tag::Closure,
        // FLAG_STATIC_LITERAL.
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = mid as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// Extern face of [`builtin_method_cell`] for the staticlib boundary —
/// torajs-meta's error-proto install consumes it to define
/// `Error.prototype.toString` as an own function entry (RFC
/// 20260718-builtin-error-ctor-first-class 刀 4). Immortal cell; the
/// dynobj define's rc stake no-ops on the static flag.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_builtin_method_cell(mid: i64) -> *mut u8 {
    builtin_method_cell(mid)
}

/// The reified `get size` getter cells — one per proto family
/// (index 0 = Map proto tag 11, 1 = Set proto tag 12) because the
/// spec getters are DISTINCT function objects per prototype. Same
/// immortal closure shape as [`builtin_method_cell`]; the carried
/// id [`torajs_rc::ANY_METHOD_GET_SIZE`] routes a `.call(recv)`
/// through the mapset dispatcher's size arm, and `.name`/`.length`
/// reads resolve "get size"/0 off the meta row.
static SIZE_GETTER_CELLS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// The getter cell for a Map (proto tag 11) or Set (proto tag 12)
/// `size` accessor — lazily allocated, immortal. Callers guarantee
/// `proto_tag` is 11 or 12.
pub(crate) fn size_getter_cell(proto_tag: i64) -> *mut u8 {
    let slot = &SIZE_GETTER_CELLS[(proto_tag - 11) as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below —
    // same layout as builtin_method_cell.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = torajs_rc::ANY_METHOD_GET_SIZE as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// The interned `.name` Str cell for a method id — lazily
/// allocated, immortal (`FLAG_STATIC_LITERAL`), Latin-1 payload
/// (method names are ASCII).
fn builtin_method_name_cell(mid: i64, name: &'static str) -> *mut u8 {
    let slot = &METHOD_NAME_CELLS[mid as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = mint_immortal_str(name.as_bytes());
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

/// Mint an immortal (`FLAG_STATIC_LITERAL`) Latin-1 Str cell — rc
/// traffic no-ops, the cycle collector skips it, it never drops.
/// Shared by the method-name intern table above and `name_get`'s
/// dynamic-key fn-name intern list (chunk D, RFC 20260711).
pub(crate) fn mint_immortal_str(name: &[u8]) -> *mut u8 {
    // SAFETY: fresh allocation sized for the 16-byte Str prefix +
    // payload, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(STR_DATA_OFF + name.len(), 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Str as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL | STR_FLAG_IS_LATIN1;
        *(cell.add(STR_LEN_OFF) as *mut u32) = name.len() as u32;
        if !name.is_empty() {
            core::ptr::copy_nonoverlapping(name.as_ptr(), cell.add(STR_DATA_OFF), name.len());
        }
        cell
    }
}

/// The reflection name of a reified method cell, `None` for
/// ordinary closures — the inspect Closure arms print
/// `[Function: <name>]` from this ahead of the fn-addr registry
/// lookup (a method cell's `fn_addr` is the throwing native entry,
/// never a table hit).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn builtin_method_name(ptr: *mut c_void) -> Option<&'static str> {
    if let Some(name) = unsafe { ns_static::ns_static_name(ptr) } {
        return Some(name);
    }
    let mid = unsafe { builtin_method_mid(ptr) }?;
    torajs_rc::any_method_meta(mid).map(|(name, _)| name)
}

/// The interned `.name` Str cell of a reified method cell, `None`
/// for ordinary closures — `name_get`'s Closure arm hands the
/// immortal cell out under the owned protocol (drop no-ops on the
/// static flag).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn builtin_method_name_cell_of(ptr: *mut c_void) -> Option<*mut u8> {
    if let Some(cell) = unsafe { ns_static::ns_static_name_cell_of(ptr) } {
        return Some(cell);
    }
    let mid = unsafe { builtin_method_mid(ptr) }?;
    let (name, _) = torajs_rc::any_method_meta(mid)?;
    Some(builtin_method_name_cell(mid, name))
}

/// The ES-spec `length` of a reified method cell, `None` for
/// ordinary closures (the env cell carries no arity field — that
/// side stays the recorded boundary).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn builtin_method_arity(ptr: *mut c_void) -> Option<u32> {
    if let Some(arity) = unsafe { ns_static::ns_static_arity(ptr) } {
        return Some(arity);
    }
    let mid = unsafe { builtin_method_mid(ptr) }?;
    torajs_rc::any_method_meta(mid).map(|(_, arity)| arity)
}

/// The method id a reified cell carries — `None` for ordinary
/// closures (discriminated by the boxed entry's address).
pub(crate) unsafe fn builtin_method_mid(ptr: *mut c_void) -> Option<i64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == bare_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64)
        } else {
            None
        }
    }
}

/// Staticlib face of [`builtin_method_mid`] — `-1` for anything
/// that is not an interned builtin-method cell. torajs-dynobj's
/// accessor-pair invoke probes its faces here (RFC
/// 20260718-accessor-reify 刀 1): a builtin cell's boxed dual entry
/// is the bare-receiver throw, so the pair invoke must re-route
/// through the mid dispatcher instead of jumping into it.
///
/// # Safety
/// `p` is null or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_method_face_mid(p: *const c_void) -> i64 {
    if p.is_null() {
        return -1;
    }
    unsafe {
        if (p.cast::<u8>().add(4) as *const u16).read() != Tag::Closure as u16 {
            return -1;
        }
        builtin_method_mid(p as *mut c_void).unwrap_or(-1)
    }
}

/// Invoke a builtin-method mid against `recv` — the accessor-pair
/// twin of the `.call` re-dispatch (same inner, no name bytes / no
/// receiver write-back). A mid the receiver's arm doesn't know
/// throws the not-callable TypeError (never reached for the
/// universal accessor mids the pairs carry today).
///
/// # Safety
/// `recv` / `argv` carry valid AnyValue bit patterns; `argv` has
/// `argc` readable slots (null iff `argc == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_method_face_dispatch(
    recv: u64,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> u64 {
    let r = unsafe {
        crate::method_call::any_method_call_inner(
            recv,
            mid,
            core::ptr::null(),
            core::ptr::null_mut(),
            argv,
            argc,
        )
    };
    if r == crate::method_call::ANY_METHOD_NO_SUCH {
        return unsafe { crate::method_call::not_callable() };
    }
    r
}

/// Member-name → interned method cell, `None` when the name is not
/// a method the receiver's dispatch arm supports (the member read
/// stays undefined, matching bun's wrong-arm answer).
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn builtin_method_lookup(recv: AnyValue, key: *const c_void) -> Option<*mut u8> {
    let mid = unsafe { key_method_id(key) };
    if mid == ANY_METHOD_UNKNOWN || (mid as usize) >= TABLE_SIZE {
        return None;
    }
    if !builtin_method_supported(recv, mid) {
        return None;
    }
    // §24.2.4.8 — Set.prototype.keys IS the values function object;
    // reads through a live Set receiver hand out the values cell so
    // `s.keys === s.values` holds.
    let mid = if mid == torajs_rc::ANY_METHOD_KEYS && is_set_cell(recv) {
        torajs_rc::ANY_METHOD_VALUES
    } else {
        mid
    };
    Some(builtin_method_cell(mid))
}

/// True iff the boxed value is a live `Tag::Set` heap cell.
fn is_set_cell(recv: AnyValue) -> bool {
    if !is_cell(recv) {
        return false;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer with a header.
    unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Set as u16 }
}

/// Read the key Str's bytes and intern them through the shared
/// compile-time table.
pub(crate) unsafe fn key_method_id(key: *const c_void) -> i64 {
    if key.is_null() {
        return ANY_METHOD_UNKNOWN;
    }
    unsafe {
        let len = (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() as usize;
        let bytes = core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len);
        match core::str::from_utf8(bytes) {
            Ok(s) => any_method_id(s),
            Err(_) => ANY_METHOD_UNKNOWN,
        }
    }
}

/// Exact per-receiver-shape support table — one arm per
/// `method_call_*` dispatch module, listing the ids that arm
/// resolves (extend together when an arm grows a method).
pub(crate) fn builtin_method_supported(recv: AnyValue, mid: i64) -> bool {
    // chunk D-1 — the universal own-property probes resolve on every
    // receiver shape (Object.prototype methods; primitives coerce
    // through ToObject and simply answer false-valued Bools).
    // valueOf joins them (§20.1.4.7 — identity on every cell, the
    // immediate itself on primitives; the dispatcher's universal
    // arm makes it callable everywhere).
    // isPrototypeOf joins them for the same reason (§20.1.3.3 — an
    // Object.prototype method the dispatcher already answers on every
    // receiver); it was callable but not readable, so `typeof
    // o.isPrototypeOf` needed a name-based shortcut in ssa_lower to
    // avoid saying "undefined".
    if mid == torajs_rc::ANY_METHOD_HAS_OWN_PROPERTY
        || mid == torajs_rc::ANY_METHOD_PROPERTY_IS_ENUMERABLE
        || mid == torajs_rc::ANY_METHOD_VALUE_OF
        || mid == torajs_rc::ANY_METHOD_IS_PROTOTYPE_OF
    {
        return true;
    }
    if is_short_str(recv) {
        return str_supports(mid);
    }
    if is_int32(recv) || is_double(recv) {
        return num_supports(mid);
    }
    if is_bool(recv) {
        return mid == ANY_METHOD_TO_STRING || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING;
    }
    if !is_cell(recv) {
        return false;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => str_supports(mid),
        t if t == Tag::Arr as u16 => arr_supports(mid),
        t if t == Tag::Map as u16 => map_supports(mid),
        t if t == Tag::Set as u16 => set_supports(mid),
        t if t == Tag::MapIter as u16 => mid == ANY_METHOD_NEXT,
        t if t == Tag::Date as u16 => date_supports(mid),
        t if t == Tag::RegExp as u16 => regexp_supports(mid),
        t if t == Tag::WeakMap as u16 => weakmap_supports(mid),
        t if t == Tag::WeakSet as u16 => weakset_supports(mid),
        t if t == Tag::Closure as u16 => closure_supports(mid),
        // Plain objects (dynobj / static-layout struct) reach the
        // dispatcher's Object.prototype toLocaleString arm plus the
        // Annex B §B.2.2.2-5 legacy accessor four, which they inherit
        // from Object.prototype like any other object — the dispatcher
        // has answered those calls since RFC
        // 20260713-annexb-legacy-accessor, but reading one as a value
        // (`typeof o.__defineGetter__`, `f.call(o, …)`) went through
        // here and said undefined. Their remaining methods resolve by
        // name probe, not by mid.
        // §20.4.3 Symbol.prototype — toString (the
        // SymbolDescriptiveString arm) plus the inherited
        // Object.prototype toLocaleString; valueOf answered by the
        // universal arm above. Reading either as a value hands out
        // the interned cell so `typeof s.toString` says "function".
        t if t == Tag::Symbol as u16 => {
            mid == ANY_METHOD_TO_STRING || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING
        }
        t if t == Tag::DynObj as u16 || t == Tag::Obj as u16 => {
            // toString sits beside toLocaleString: both are inherited
            // from Object.prototype and the dispatcher has always
            // answered the call (`({}).toString()` is
            // "[object Object]"), but only toLocaleString was
            // declared here, so reading the same method as a value
            // (`const m = o.toString`) came back undefined.
            mid == ANY_METHOD_TO_STRING
                || mid == torajs_rc::ANY_METHOD_TO_LOCALE_STRING
                || (torajs_rc::ANY_METHOD_DEFINE_GETTER..=torajs_rc::ANY_METHOD_LOOKUP_SETTER)
                    .contains(&mid)
        }
        _ => false,
    }
}
