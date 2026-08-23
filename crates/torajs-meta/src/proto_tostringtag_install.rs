//! `@@toStringTag` own entries on the builtin prototypes that the
//! spec gives one.
//!
//! Nine prototypes carry the tag as a plain data property with
//! `{ [[Writable]]: false, [[Enumerable]]: false,
//! [[Configurable]]: true }` — Symbol §20.4.3.5, BigInt §21.2.3.5,
//! Promise §27.2.5.5, Map §24.1.3.14, Set §24.2.3.13, WeakMap
//! §24.3.3.4, WeakSet §24.4.3.5, WeakRef §26.1.3.3, ArrayBuffer
//! §25.1.6.5. Everything else
//! (Object / Array / String / Number / Boolean / Function / Date /
//! RegExp / Error) has NO tag: its badge comes from the builtinTag
//! walk instead, which is why installing one there would be wrong
//! rather than merely redundant.
//!
//! Before this module those eight badges were emulated in
//! `method_call_object_proto::cell_badge`, keyed on which prototype
//! singleton the pointer was. That answered the badge correctly and
//! left the property absent, so the two questions disagreed:
//! `Object.prototype.toString.call(Map.prototype)` said "[object
//! Map]" while `getOwnPropertyDescriptor(Map.prototype,
//! Symbol.toStringTag)` said undefined. The tag is configurable, so
//! the emulation also survived its own deletion.
//!
//! Called once per prototype from the singleton mint
//! (`torajs-rc::builtin_proto`) before the CAS install — a race
//! loser leaks its fresh entry along with the dynobj, the same
//! posture as the `object_proto_install` / `function_proto_install`
//! siblings.

use core::ffi::c_void;

use crate::reflect::ANY_HEAP;

/// Flag-byte mirror of `torajs_dynobj::layout::DEFINE_*`. Every
/// attribute is stated so the entry says what it is rather than
/// inheriting a write's defaults.
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;

/// Index of `Symbol.toStringTag` in torajs-str's alphabetical
/// well-known table.
const WK_TO_STRING_TAG: i64 = 13;

unsafe extern "C" {
    /// torajs-str — the idx-th §6.1.5.1 well-known symbol (owned +1).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-str — pooled Str allocation (header + len prefix).
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    /// torajs-dynobj — define kernel (§10.1.6.3 apply core).
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
}

/// The tag a builtin-prototype slot carries, or `None` when the spec
/// gives that prototype none. Tag numbering mirrors
/// `torajs_rc::builtin_proto`'s slot order.
fn tag_for_proto(idx: i64) -> Option<&'static [u8]> {
    match idx {
        5 => Some(b"Symbol"),
        6 => Some(b"BigInt"),
        10 => Some(b"Promise"),
        11 => Some(b"Map"),
        12 => Some(b"Set"),
        // §25.1.6.5 — a plain data property like the eight above.
        // Its typed-array neighbours are NOT here on purpose:
        // §23.2.3.32 gives %TypedArray%.prototype an ACCESSOR that
        // brand-checks its receiver, so `Int8Array.prototype[
        // @@toStringTag]` is undefined while an instance's is
        // "Int8Array". A data entry on each of the eleven per-kind
        // prototypes would answer the instance right and the
        // prototype wrong, and the abstract %TypedArray%.prototype
        // the real accessor lives on is a recorded gap in RFC
        // 20260823-typedarray-substrate.
        19 => Some(b"ArrayBuffer"),
        16 => Some(b"WeakMap"),
        17 => Some(b"WeakSet"),
        18 => Some(b"WeakRef"),
        _ => None,
    }
}

/// Define `@@toStringTag` on a freshly minted builtin prototype, if
/// its spec clause gives it one. A no-op for the rest.
///
/// # Safety
/// `proto` points to a valid, freshly minted TAG_DYNOBJ cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_tostringtag_install(proto: *mut c_void, idx: i64) {
    let Some(tag) = tag_for_proto(idx) else {
        return;
    };
    unsafe {
        let key = __torajs_symbol_well_known(WK_TO_STRING_TAG);
        if key.is_null() {
            return;
        }
        // The value is a fresh Str the entry keeps; the well-known
        // symbol is a process-lifetime singleton, so its +1 is never
        // given back (the ns_object namespace fills do the same).
        let value = __torajs_str_alloc_pooled(tag.len() as u64);
        core::ptr::copy_nonoverlapping(tag.as_ptr(), value.add(16), tag.len());
        let flags = DEFINE_FLAG_CONFIGURABLE
            | DEFINE_PRESENT_WRITABLE
            | DEFINE_PRESENT_ENUMERABLE
            | DEFINE_PRESENT_CONFIGURABLE
            | DEFINE_PRESENT_VALUE;
        let mut slot = proto;
        __torajs_dynobj_define(
            &mut slot,
            key as *const u8,
            ANY_HEAP as u64,
            value as u64,
            flags,
        );
    }
}
