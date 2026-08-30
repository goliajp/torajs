//! §10.4.6 module namespace exotic object — the SHAPE half.
//!
//! A namespace materializes as an ordinary dynobj (the resolver's
//! synthetic object literal). Five of the exotic internal methods are
//! not new behavior at all once that object carries the right
//! attributes, because the ordinary ones already answer them:
//!
//! - §10.4.6.1 `[[GetPrototypeOf]]` → null: the null-prototype bit;
//! - §10.4.6.3 `[[IsExtensible]]` → false and §10.4.6.4
//!   `[[PreventExtensions]]` → true: the non-extensible bit, which is
//!   also what makes §10.4.6.6 refuse a define of a NEW key;
//! - §10.4.6.5 `[[GetOwnProperty]]`'s `[[Configurable]]: false` on
//!   every export: the per-entry seal, which is in turn what makes
//!   §10.4.6.10 `[[Delete]]` of an export fail;
//! - §10.4.6.11 `[[OwnPropertyKeys]]`'s trailing symbol keys and the
//!   `[object Module]` badge: the `@@toStringTag` own entry, a
//!   `{ w: false, e: false, c: false }` data property per §10.4.6.12
//!   step 8.
//!
//! What is NOT here is the half that has no ordinary spelling:
//! §10.4.6.9 `[[Set]]` returns false even though the exports are
//! writable, and §10.4.6.6 refuses a redefine of an EXISTING key that
//! an ordinary non-configurable-but-writable entry would accept. Those
//! need the receiver to be recognizable as a namespace at the write,
//! which is a flag bit rather than an attribute — a separate knife.
//!
//! Called once per namespace right after its object literal lowers
//! (`ssa_lower_stmt_let_decl`), so an importer that never uses the
//! namespace as a value pays nothing: the direct-connect pass
//! (`ast::module_ns_members`) has already routed every static
//! `ns.<export>` read past the object.

use core::ffi::c_void;

use crate::reflect::ANY_HEAP;

/// Flag-byte mirror of `torajs_dynobj::layout::DEFINE_*`. Every
/// attribute is stated so the entry says what it is rather than
/// inheriting a write's defaults; §10.4.6.12 step 8 gives the tag
/// `{ [[Writable]]: false, [[Enumerable]]: false,
/// [[Configurable]]: false }`, so no VALUE bit is set — only the
/// PRESENT bits that make the three attributes explicit.
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;

/// Index of `Symbol.toStringTag` in torajs-str's alphabetical
/// well-known table (mirror of the `proto_tostringtag_install`
/// sibling's constant).
const WK_TO_STRING_TAG: i64 = 13;

unsafe extern "C" {
    /// torajs-str — the idx-th §6.1.5.1 well-known symbol (owned +1).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-str — pooled Str allocation (header + len prefix).
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    /// torajs-dynobj — define kernel (§10.1.6.3 apply core).
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-dynobj — clear `[[Configurable]]` on every live entry.
    fn __torajs_dynobj_seal_entries(obj: *mut c_void);
    /// torajs-dynobj — set the null-prototype header bit.
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    /// torajs-rc — set the non-extensible header bit.
    fn __torajs_obj_prevent_extensions(p: *mut c_void) -> *mut c_void;
    /// torajs-anyvalue — NaN-box readers.
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `__torajs_module_ns_finalize(ns)` — give a freshly lowered
/// namespace object literal the §10.4.6 attributes. Takes the boxed
/// value the lowering already has in hand; a non-heap box is a no-op.
///
/// Idempotent: a second call redefines the same tag entry with the
/// same attributes and re-sets two already-set bits.
///
/// # Safety
/// `ns` is a NaN-boxed value; when its tag says heap, the payload is
/// a live dynobj the caller owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_ns_finalize(ns: u64) {
    unsafe {
        if __torajs_anyv_unbox_tag(ns) != ANY_HEAP {
            return;
        }
        let obj = __torajs_anyv_unbox_value(ns) as *mut c_void;
        if obj.is_null() {
            return;
        }
        // Order matters: the tag entry has to land BEFORE the object
        // stops being extensible, and the seal walk has to run after
        // the tag entry exists so it covers it too (it is already
        // non-configurable — the walk is what keeps the two halves
        // from disagreeing if the define defaults ever move).
        let key = __torajs_symbol_well_known(WK_TO_STRING_TAG);
        if !key.is_null() {
            // The value is a fresh Str the entry keeps; the
            // well-known symbol is a process-lifetime singleton, so
            // its +1 is never given back (the builtin-prototype tag
            // install does the same).
            let tag = b"Module";
            let value = __torajs_str_alloc_pooled(tag.len() as u64);
            core::ptr::copy_nonoverlapping(tag.as_ptr(), value.add(16), tag.len());
            let mut slot = obj;
            __torajs_dynobj_define_plain(
                &mut slot,
                key as *const u8,
                ANY_HEAP as u64,
                value as u64,
                DEFINE_PRESENT_WRITABLE
                    | DEFINE_PRESENT_ENUMERABLE
                    | DEFINE_PRESENT_CONFIGURABLE
                    | DEFINE_PRESENT_VALUE,
            );
        }
        __torajs_dynobj_seal_entries(obj);
        __torajs_dynobj_mark_null_proto(obj);
        __torajs_obj_prevent_extensions(obj);
    }
}
