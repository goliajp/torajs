//! `Object.prototype.__proto__` accessor install (RFC
//! 20260718-accessor-reify 刀 1).
//!
//! Annex B §B.2.2.1 makes `__proto__` an accessor property of
//! %Object.prototype% — `{ get, set, enumerable: false,
//! configurable: true }`. The member-read/-write semantics have
//! their own routes (reflect_proto / reflect_proto_set); this
//! module supplies the REFLECTION face: a real AccessorPair own
//! entry whose get/set slots hold the interned builtin-method
//! cells for the `get __proto__` / `set __proto__` mids, so
//! `getOwnPropertyDescriptor` / `getOwnPropertyNames` /
//! `__lookupGetter__` answer through the ordinary own-entry
//! machinery with no per-surface special case.
//!
//! Called once per process from the proto-singleton mint
//! (`torajs-rc::builtin_proto`, tag 1) before the CAS install — a
//! benign CAS-race loser leaks its fresh dynobj entry along with
//! the dynobj itself (same posture as the singleton allocation).

use core::ffi::c_void;

use crate::reflect::{ANY_HEAP, alloc_str_key};

/// Mirror of `torajs_rc::ANY_METHOD_PROTO_GET` / `_SET` across the
/// staticlib boundary (the `ANY_METHOD_TO_STRING_MID` precedent).
const ANY_METHOD_PROTO_GET_MID: i64 = 153;
const ANY_METHOD_PROTO_SET_MID: i64 = 154;

/// Kinds mirror of `torajs_dynobj::accessor::ACC_KIND_BOXED` on
/// both faces — the cells carry the boxed dual-entry sentinel; the
/// member surfaces never invoke through the pair (the `__proto__`
/// key routes to its dedicated read/write paths first), only the
/// reflection surfaces hand the faces out as values.
const ACC_KINDS_BOXED_BOTH: u64 = 5 | (5 << 8);

/// Flag-byte mirror of `torajs_dynobj::layout::DEFINE_*` — the
/// §B.2.2.1 descriptor is `{ enumerable: false, configurable:
/// true }` with both accessor faces present.
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
const DEFINE_PRESENT_GET: u64 = 1 << 7;
const DEFINE_PRESENT_SET: u64 = 1 << 8;

unsafe extern "C" {
    /// torajs-anyvalue — the IN-CELL slot holding
    /// %Function.prototype%'s expando dynobj (the singleton is a
    /// Closure cell, §20.2.3). A slot pointer, so the define kernel's
    /// relocation lands back in the cell.
    fn __torajs_function_proto_props_slot(proto: *mut c_void) -> *mut *mut c_void;
    /// torajs-anyvalue — interned immortal cell for a method id.
    fn __torajs_builtin_method_cell(mid: i64) -> *mut u8;
    /// torajs-str — the idx-th well-known symbol singleton (immortal;
    /// the define's stake on it is never given back).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-dynobj — fresh `+1`-rc AccessorPair (faces transfer;
    /// the immortal cells' rc traffic no-ops on the static flag).
    fn __torajs_accessor_pair_new(get: *mut c_void, set: *mut c_void, kinds: u64) -> *mut c_void;
    /// torajs-dynobj — define kernel (§10.1.6.3 apply core).
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_str_drop(s: *mut u8);
}

/// Define the `__proto__` accessor own entry on a fresh
/// %Object.prototype% dynobj. The pair's +1 transfers into the
/// entry; the two faces are interned immortals.
///
/// # Safety
/// `proto` points to a valid, freshly minted TAG_DYNOBJ cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_proto_install(proto: *mut c_void) {
    unsafe {
        let get_cell = __torajs_builtin_method_cell(ANY_METHOD_PROTO_GET_MID);
        let set_cell = __torajs_builtin_method_cell(ANY_METHOD_PROTO_SET_MID);
        // The singleton IS its own entry table here, and it is
        // registered by address, so a relocation would strand it —
        // one entry into a fresh dynobj does not grow, which is why a
        // local slot was ever sound. Spelled as a slot anyway, so the
        // helper has one contract.
        let mut slot = proto;
        install_accessor_entry(&mut slot, b"__proto__", get_cell, set_cell);
    }
}

/// Mirror of `torajs_rc::ANY_METHOD_THROW_TYPE_ERROR`.
const ANY_METHOD_THROW_TYPE_ERROR_MID: i64 = 155;

/// §10.2.4 AddRestrictedFunctionProperties — `caller` / `arguments`
/// accessor own entries on a fresh %Function.prototype% dynobj.
/// All four faces are the ONE interned %ThrowTypeError% cell, so
/// `callerDesc.get === argumentsDesc.set` (four-way identity).
///
/// # Safety
/// `proto` points to a valid, freshly minted TAG_DYNOBJ cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_function_proto_install(proto: *mut c_void) {
    unsafe {
        // §20.2.3 makes %Function.prototype% a built-in FUNCTION
        // object, so the singleton is a Closure cell and its own
        // entries live in the expando the mint pre-allocated. The
        // narrow define kernel below contracts for a plain dynobj
        // receiver; handing it the closure cell wrote entry records
        // over `fn_addr`.
        let slot = __torajs_function_proto_props_slot(proto);
        let thrower = __torajs_builtin_method_cell(ANY_METHOD_THROW_TYPE_ERROR_MID);
        install_accessor_entry(slot, b"caller", thrower, thrower);
        install_accessor_entry(slot, b"arguments", thrower, thrower);
        // §20.2.3.6 — the default `instanceof` handler, and the one
        // own property here the spec locks down completely
        // ({W,E,C} all false). It was the operator's fall-through
        // shape before, which is a different thing from an entry:
        // `f[Symbol.hasInstance]` answered undefined and
        // `getOwnPropertySymbols(Function.prototype)` was empty,
        // while the behaviour it stands for worked.
        let key = __torajs_symbol_well_known(WK_HAS_INSTANCE);
        if !key.is_null() {
            __torajs_dynobj_define_plain(
                slot,
                key.cast::<u8>(),
                ANY_HEAP as u64,
                __torajs_builtin_method_cell(ANY_METHOD_FN_HAS_INSTANCE_MID) as u64,
                DEFINE_PRESENT_VALUE
                    | DEFINE_PRESENT_WRITABLE
                    | DEFINE_PRESENT_ENUMERABLE
                    | DEFINE_PRESENT_CONFIGURABLE,
            );
        }
    }
}

/// Mirror of `torajs_rc::ANY_METHOD_FN_HAS_INSTANCE`.
const ANY_METHOD_FN_HAS_INSTANCE_MID: i64 = 205;

/// §6.1.5.1 well-known index of `@@hasInstance` — the table is
/// alphabetical (`method_value::symbol_static` mirror).
const WK_HAS_INSTANCE: i64 = 3;

/// One `{get, set, enumerable: false, configurable: true}` accessor
/// own entry from interned immortal faces (each pair takes its own
/// fresh AccessorPair; the faces' rc traffic no-ops).
unsafe fn install_accessor_entry(
    obj_slot: *mut *mut c_void,
    key_bytes: &[u8],
    get_cell: *mut u8,
    set_cell: *mut u8,
) {
    unsafe {
        let pair = __torajs_accessor_pair_new(
            get_cell as *mut c_void,
            set_cell as *mut c_void,
            ACC_KINDS_BOXED_BOTH,
        );
        let flags = DEFINE_PRESENT_VALUE
            | DEFINE_PRESENT_GET
            | DEFINE_PRESENT_SET
            | DEFINE_PRESENT_ENUMERABLE
            | DEFINE_PRESENT_CONFIGURABLE
            | DEFINE_FLAG_CONFIGURABLE;
        let key = alloc_str_key(key_bytes);
        __torajs_dynobj_define_plain(obj_slot, key, ANY_HEAP as u64, pair as u64, flags);
        __torajs_str_drop(key);
    }
}
