//! Array index accessor properties — RFC
//! 20260713-defprop-residual-cluster chunk C.
//!
//! `Object.defineProperty(arr, "0", { get, set })` stores the
//! AccessorPair in the index's shadow entry (the expando props
//! dynobj, canonical index key) using the SAME storage convention as
//! a plain dynobj accessor entry — `dynobj_define` with the
//! `DEFINE_PRESENT_GET/SET` bits handles the pair store, the
//! §10.1.6.3 redefine validation, and the chunk-D face merge. The
//! element-storage slot goes dead (`undefined`), mirroring the hole
//! shape; readers and writers gate on `FLAG_ARR_EXOTIC_INDEX` and
//! probe the shadow entry: `dynobj_get_tag == ANY_ACCESSOR` routes
//! `arr[i]` reads through the getter and `arr[i] = v` writes through
//! the setter. gOPD rides the existing expando-walk delegation (the
//! shadow entry IS a dynobj accessor entry, so the TAG_DYNOBJ arm
//! reports `{get, set, enumerable, configurable}` unchanged).
//!
//! Scope gate: only `Array<Any>` element storage (every test262
//! receiver reaches here as `var arr = []`). A typed-slot array
//! declines (recorded divergence — its element reads are inline
//! codegen with no accessor hook).

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ANY, FLAG_ARR_EXOTIC_INDEX, FLAG_NON_EXTENSIBLE, HeapHeader};

use crate::define::{ANY_UNDEF, F_HOLE, index_flags_with_key, mint_index_key, props_slot};
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-dynobj — boolean-answer flavor (rotation 267 刀 R5b):
    /// refusal answers 0 with no pending throw.
    fn __torajs_dynobj_define_soft(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    ) -> i64;
    fn __torajs_dynobj_delete(dynobj: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    // Signature shape mirrors props.rs's prior declarations (same
    // crate — extern redeclarations must agree).
    fn __torajs_dynobj_get_tag(obj: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    fn __torajs_accessor_invoke_setter(pair: *const c_void, recv_anyv: u64, value_anyv: u64)
    -> i32;
}

/// `dynobj_get_tag` accessor sentinel (`torajs_dynobj::layout::ANY_ACCESSOR`).
const ANY_ACCESSOR: u64 = 6;
/// `AnySlotTag::Heap` mirror.
const ANY_HEAP: u64 = 4;
/// Descriptor `flags_byte` present bits (torajs-dynobj layout mirror).
const P_ENUMERABLE: u64 = 1 << 4;
const P_CONFIGURABLE: u64 = 1 << 5;
const F_ENUMERABLE: u64 = 1 << 1;
const F_CONFIGURABLE: u64 = 1 << 2;

/// §10.4.2.1 accessor-descriptor arm of `define_index` — `(tag,
/// value)` carry the AccessorPair (one transferred rc), `flags_byte`
/// has `DEFINE_PRESENT_GET` and/or `_SET`. Answers 1 on success, 0
/// on a §10.1.6.3 refusal (recorded as a TypeError only for the
/// throwing flavor — rotation 267 刀 R5b).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str. Caller
/// must check for a pending throw after return.
pub(crate) unsafe fn define_index_accessor(
    arr: *mut c_void,
    key: *mut c_void,
    idx: u64,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_ARR_ANY == 0 {
        // Typed element storage — no accessor hook on the inline read
        // path (recorded divergence, same reflection boundary as the
        // typed-struct define no-op; the boolean flavor answers 0 —
        // nothing was defined).
        unsafe { drop_owned_pair(tag, value) };
        return if throw_on_refusal { 1 } else { 0 };
    }
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };

    if idx >= len {
        // Fresh append — §10.4.2.1 step 2: locked length rejects the
        // implicit length bump; then the extensible check.
        if header.flags & torajs_rc::FLAG_ARR_LENGTH_RO != 0 {
            return unsafe {
                refuse_pair(
                    throw_on_refusal,
                    c"Attempting to define property beyond a non-writable array length.".as_ptr(),
                    tag,
                    value,
                )
            };
        }
        if header.flags & FLAG_NON_EXTENSIBLE != 0 {
            return unsafe {
                refuse_pair(
                    throw_on_refusal,
                    c"Attempting to define property on object that is not extensible.".as_ptr(),
                    tag,
                    value,
                )
            };
        }
        // Dense model: gap-fill + a dead undefined slot for the
        // accessor index (the value lives behind the getter).
        let mut cursor = len;
        while cursor <= idx {
            unsafe { crate::__torajs_arr_push_any(arr, ANY_UNDEF, 0) };
            cursor += 1;
        }
        // The gap the fill just walked is HOLES, not own undefineds:
        // §10.4.2.1 raises `length` to cover the defined index and
        // creates nothing in between. The plain-write grow path marks
        // them for the same reason; this one did not, so
        // `Object.defineProperty(Array.prototype, "1", …)` on the empty
        // prototype array left index 0 answering own — visible through
        // `hasOwnProperty` and, once a hole gate consults it, through
        // every callback method on every array that inherits from it.
        if idx > len {
            unsafe { crate::define_hole::mark_hole_range(arr, len, idx) };
        }
        return unsafe {
            define_shadow_accessor(arr, key, tag, value, flags_byte, throw_on_refusal)
        };
    }

    let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    if cur_flags & F_HOLE != 0 {
        // A hole is an absent property — fresh CreateProperty over the
        // extensible check; clear the sentinel entry outright so the
        // define lands as a fresh create (absent attributes complete
        // to false), not a revive-then-redefine.
        if header.flags & FLAG_NON_EXTENSIBLE != 0 {
            return unsafe {
                refuse_pair(
                    throw_on_refusal,
                    c"Attempting to define property on object that is not extensible.".as_ptr(),
                    tag,
                    value,
                )
            };
        }
        let props = unsafe { *props_slot(arr) };
        if !props.is_null() {
            unsafe { __torajs_dynobj_delete(props, key as *const c_void) };
        }
        return unsafe {
            define_shadow_accessor(arr, key, tag, value, flags_byte, throw_on_refusal)
        };
    }

    // Existing index. §10.1.6.3 step 5 — a data → accessor transition
    // needs the current property configurable, and the gate must run
    // BEFORE the element slot is cleared (a late reject must not lose
    // the data). A data shadow entry stores flags only (its value slot
    // is dead undefined), so the tag probe cannot distinguish it from
    // "no entry" — the flags probe already answered for both, and key
    // PRESENCE (`dynobj_has`) decides whether `dynobj_define` sees a
    // current entry to merge absent fields from.
    let props = unsafe { *props_slot(arr) };
    let entry_tag = if props.is_null() {
        ANY_UNDEF
    } else {
        unsafe { __torajs_dynobj_get_tag(props, key as *const c_void) }
    };
    if entry_tag != ANY_ACCESSOR && cur_flags & F_CONFIGURABLE == 0 {
        return unsafe {
            refuse_pair(
                throw_on_refusal,
                c"Attempting to change configurable attribute of unconfigurable property.".as_ptr(),
                tag,
                value,
            )
        };
    }
    // When no shadow entry exists the index is an implicit data
    // property {w:1, e:1, c:1} — a redefine to accessor keeps the
    // CURRENT e/c for absent descriptor fields (§10.1.6.3), so inject
    // them as present bits before the fresh create.
    let has_entry = !props.is_null()
        && unsafe { __torajs_dynobj_has(props as *const c_void, key as *const c_void) } != 0;
    let mut flags = flags_byte;
    if !has_entry {
        if flags & P_ENUMERABLE == 0 {
            flags |= P_ENUMERABLE | F_ENUMERABLE;
        }
        if flags & P_CONFIGURABLE == 0 {
            flags |= P_CONFIGURABLE | F_CONFIGURABLE;
        }
    }
    // The element slot goes dead — the value now lives behind the
    // getter. (Already-accessor indexes hold undefined here; the
    // double store is harmless.)
    unsafe { clear_element_slot(arr, idx) };
    unsafe { define_shadow_accessor(arr, key, tag, value, flags, throw_on_refusal) }
}

/// Shared refusal answer — release the transferred pair rc, record
/// the TypeError only for the throwing flavor, answer 0.
unsafe fn refuse_pair(
    throw_on_refusal: bool,
    msg: *const core::ffi::c_char,
    tag: u64,
    value: u64,
) -> i64 {
    unsafe { drop_owned_pair(tag, value) };
    if throw_on_refusal {
        unsafe { __torajs_throw_type_error(msg) };
    }
    0
}

/// Shared tail — route the pair through the dynobj define kernel
/// (create or validate-and-merge, flavor-matched) on the
/// lazily-allocated props dynobj, then raise the header exotic bit
/// so readers/writers probe the shadow.
unsafe fn define_shadow_accessor(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    let ok = if throw_on_refusal {
        unsafe { __torajs_dynobj_define(slot, key, tag, value, flags_byte) };
        1
    } else {
        unsafe { __torajs_dynobj_define_soft(slot, key, tag, value, flags_byte) }
    };
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
    ok
}

/// Drop the Array<Any> element slot's current value and store a
/// NaN-box `undefined`.
unsafe fn clear_element_slot(arr: *mut c_void, idx: u64) {
    let slot = unsafe { crate::any::slot_anyvalue_ptr(arr as *mut u8, idx) };
    unsafe { __torajs_value_drop_heap((*slot) as *mut c_void) };
    unsafe { *slot = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0) };
}

/// Release a transferred `ANY_HEAP` rc on a rejected define.
unsafe fn drop_owned_pair(tag: u64, value: u64) {
    if tag == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// The index's AccessorPair, or NULL when the index is not an
/// accessor. Callers gate on `FLAG_ARR_EXOTIC_INDEX` (checked again
/// here so an unconditional call stays correct).
///
/// # Safety
/// `arr` is NULL or a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_accessor(arr: *const c_void, idx: u64) -> *mut c_void {
    if arr.is_null() {
        return core::ptr::null_mut();
    }
    let header = unsafe { &*(arr as *const HeapHeader) };
    if header.flags & FLAG_ARR_EXOTIC_INDEX == 0 {
        return core::ptr::null_mut();
    }
    let props = unsafe { *props_slot(arr as *mut c_void) };
    if props.is_null() {
        return core::ptr::null_mut();
    }
    let key = unsafe { mint_index_key(idx) };
    let tag = unsafe { __torajs_dynobj_get_tag(props as *mut c_void, key as *const c_void) };
    let pair = if tag == ANY_ACCESSOR {
        unsafe {
            __torajs_dynobj_get_value(props as *mut c_void, key as *const c_void) as *mut c_void
        }
    } else {
        core::ptr::null_mut()
    };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    pair
}

/// `arr[i]` read through an accessor index — invoke the getter
/// (undefined when getter-less). Answers the NaN-box AnyValue.
/// `arr` is the addressed array cell — an `ACC_KIND_RECV` fn-expr
/// getter reads it as `this` (RFC 20260717-fnexpr-this-channel).
///
/// # Safety
/// `pair` is a live `AccessorPair` cell; `arr` the live array it
/// sits on.
pub(crate) unsafe fn read_via_getter(pair: *const c_void, arr: *const c_void) -> u64 {
    let recv = unsafe { __torajs_anyv_box_from_pair(4, arr as i64) };
    unsafe { __torajs_accessor_invoke_getter(pair, recv) }
}

/// `arr[i] = (tag, value)` write through an accessor index — invoke
/// the setter; a getter-only accessor raises the assignment
/// TypeError (ledger mirrors `dynobj_set`'s accessor arm).
///
/// # Safety
/// `pair` is a live `AccessorPair` cell.
pub(crate) unsafe fn write_via_setter(
    pair: *const c_void,
    arr: *const c_void,
    tag: u64,
    value: u64,
) {
    let value_anyv = unsafe { __torajs_anyv_box_from_pair(tag as i64, value as i64) };
    let recv = unsafe { __torajs_anyv_box_from_pair(4, arr as i64) };
    if unsafe { __torajs_accessor_invoke_setter(pair, recv, value_anyv) } == 0 {
        unsafe { __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr()) };
    }
}
