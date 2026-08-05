//! `Object.defineProperty(obj, key, descriptor)` — full attribute-flag
//! tracking path.
//!
//! Implements spec §10.1.6.3 ValidateAndApplyPropertyDescriptor for the
//! data-property subset (accessor descriptors not yet supported by tr).
//!
//! ## flags_byte layout
//! Low 3 bits = flag VALUE (writable / enumerable / configurable);
//! bits 3-5 = flag PRESENT in descriptor (distinguishes "absent" from
//! "present-false"); bit 6 = `[[Value]]` present.
//!
//! ## Validation rules (current.configurable=false branch)
//! - Reject upgrading configurable: false → true.
//! - Reject changing enumerable to a different value.
//! - With current.writable=false: reject upgrading writable false → true,
//!   AND reject a [[Value]] change unless SameValue (approximated via
//!   exact (tag, value) match — same heuristic as Any===Any).
//!
//! Each rejection records pending TypeError via TLS + returns — the
//! caller's ssa-lower-side `emit_throw_check` propagates. Matches
//! `feedback_throw_extern_returns_void`: throw extern is `()` not `-> !`.

use core::ffi::c_void;

use crate::layout::{
    ANY_HEAP, ANY_UNDEF, BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    BUCKET_TAG_MASK, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_VALUE, DYNOBJ_HDR_FLAG_NON_EXTENSIBLE, TAG_ARR_HDR,
};
use crate::probe::{
    Entry, bucket_make_key_tagged, count, entries, entries_cap, entries_len, index_ptr,
    key_str_bytes, probe, set_count, set_entries_len,
};
use crate::resize::resize;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_to_bool(v: u64) -> bool;
    /// torajs-arr — Array DefineOwnProperty kernel (RFC
    /// 20260712-arr-exotic-define chunk B receiver dispatch).
    fn __torajs_arr_define(
        arr: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-arr — boolean-answer flavor (rotation 267 刀 R5a):
    /// refusal answers 0 with no pending throw.
    fn __torajs_arr_define_soft(
        arr: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    ) -> i64;
    /// torajs-rc — builtin-prototype-singleton own-write note (RFC
    /// 20260721 刀 11 G13): marks the (proto tag, mid) patch bit the
    /// any-method dispatcher's primitive fast arms pre-gate on.
    fn __torajs_builtin_proto_note_own_write(obj: *const c_void, name: *const u8, len: i64);
}

/// `__torajs_dynobj_define(obj_slot, key, tag, value, flags_byte)`.
///
/// Thin extern wrapper over [`define_apply`] for the compile-time
/// literal-descriptor path (ssa_lower extracts the flags + value at
/// compile time). The runtime-descriptor path
/// (`define_from_desc.rs`) shares the same apply core.
///
/// # Safety
/// `obj_slot` is non-NULL and points at a live `*mut c_void` holding
/// a dynobj or NULL. `key` is a live Str heap pointer. `tag` / `value`
/// honored only when bit 6 (`DEFINE_PRESENT_VALUE`) of `flags_byte`
/// is set. Caller must check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    unsafe { define_apply(obj_slot, key, tag, value, flags_byte, true) };
}

/// §28.1.2 Reflect.defineProperty flavor — identical §10.1.6.3
/// validate/apply, but a refusal answers 0 with NO pending throw
/// (the TypeError belongs to Object.defineProperty's §20.1.2.4
/// caller, not to [[DefineOwnProperty]] itself).
///
/// # Safety
/// Same contract as [`__torajs_dynobj_define`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_soft(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) -> i64 {
    unsafe { define_apply(obj_slot, key, tag, value, flags_byte, false) }
}

/// Shared refusal answer — record the TypeError only for the
/// throwing flavor, release the caller-transferred [[Value]] stake
/// (gated on `has_value` — `value` is honored only under
/// `DEFINE_PRESENT_VALUE`), answer 0.
pub(crate) unsafe fn refuse(
    throw_on_refusal: bool,
    msg: *const u8,
    has_value: bool,
    tag: u64,
    value: u64,
) -> i64 {
    if throw_on_refusal {
        unsafe { __torajs_throw_type_error(msg) };
    }
    if has_value {
        unsafe { drop_rejected_value(tag, value) };
    }
    0
}

/// Release the caller-transferred `[[Value]]` stake on a rejection
/// path. Every refusal leg in [`define_apply`] /
/// [`crate::define_redefine::redefine_entry`] leaves before the store
/// that would consume the value — without this the desc value
/// stranded one rc per rejected define (`value` is honored only under
/// `DEFINE_PRESENT_VALUE`, so legs gate on `has_value`).
pub(crate) unsafe fn drop_rejected_value(tag: u64, value: u64) {
    if (tag & BUCKET_TAG_MASK) == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// Spec §10.1.6.3 ValidateAndApplyPropertyDescriptor — data-property
/// subset. Shared core for both the literal
/// ([`__torajs_dynobj_define`]) and runtime-descriptor
/// (`define_from_desc.rs`) entries. Answers 1 on success, 0 on a
/// refusal (recorded as a TypeError only when `throw_on_refusal`).
///
/// # Safety
/// Same contract as [`__torajs_dynobj_define`]. When `flags_byte` sets
/// `DEFINE_PRESENT_VALUE` and `tag == ANY_HEAP`, the caller transfers
/// one rc of `value` (consumed on store, dropped on redefine, or
/// dropped on every rejection leg via [`drop_rejected_value`]).
pub(crate) unsafe fn define_apply(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    let mut obj = unsafe { *obj_slot };
    if obj.is_null() {
        return 1;
    }
    // RFC 20260712-arr-exotic-define chunk B — an Any receiver that
    // holds an Array cell routes to the Array DefineOwnProperty
    // kernel (§10.4.2.1). Pre-fix the Arr header (`len` @8) was read
    // as a dynobj header (count/cap @8) — silent corruption. The Arr
    // cell never relocates (grow swaps the data pointer), so no
    // slot writeback is needed.
    let htag = unsafe { (obj.cast::<u8>().add(4) as *const u16).read() };
    if htag == TAG_ARR_HDR {
        if throw_on_refusal {
            unsafe { __torajs_arr_define(obj, key, tag, value, flags_byte) };
            return 1;
        }
        return unsafe { __torajs_arr_define_soft(obj, key, tag, value, flags_byte) };
    }
    // Primitive-wrapper receiver — §10.4.3.2 inherent-slot validation
    // / expando define (pre-fix the wrapper layout was walked as a
    // dynobj header: silent corruption, SIGSEGV on the first define).
    // The wrapper cell never relocates, so no slot writeback.
    if htag == crate::define_wrapper::TAG_NUMBER_WRAPPER
        || htag == crate::define_wrapper::TAG_STRING_WRAPPER
        || htag == crate::define_wrapper::TAG_BOOLEAN_WRAPPER
    {
        return unsafe {
            crate::define_wrapper::wrapper_define(
                obj,
                key,
                tag,
                value,
                flags_byte,
                throw_on_refusal,
            )
        };
    }
    // Closure receiver (T-27 Function-as-Object) — its own
    // properties live in the lazy expando dynobj at +24; recurse
    // with the expando slot so the full §10.1.6.3 validate/apply
    // (non-configurable redefine rejection included) runs against
    // the entry table. Pre-fix the closure layout was walked as a
    // dynobj header (fn_addr read as count/cap) — SIGSEGV on the
    // first defineProperty against an any-typed function.
    if htag == crate::layout::TAG_CLOSURE_HDR {
        let props_slot =
            unsafe { obj.cast::<u8>().add(crate::layout::CELL_PROPS_OFF) } as *mut *mut c_void;
        unsafe {
            if (*props_slot).is_null() {
                *props_slot = crate::alloc::__torajs_dynobj_alloc();
            }
            return define_apply(props_slot, key, tag, value, flags_byte, throw_on_refusal);
        }
    }
    // Class-instance receiver — an `any`-typed variable holding one
    // reaches here rather than through the lowering's typed arm, and
    // the two spellings have to define the same entry. Same recursion
    // as the Closure arm: the instance's own properties live in the
    // lazy expando dynobj at +24, so recurse with that slot and the
    // full §10.1.6.3 validate/apply runs against the entry table.
    //
    // A DECLARED data field takes the sibling's redefine arm instead:
    // its value stays in the layout slot and only its ATTRIBUTES land
    // in the dict, so no second own property of the same name is
    // created (RFC 20260806-declared-field-redefine). A declared
    // ACCESSOR member keeps the refusal below — the layout carries
    // those under synthetic slots and redefining one is a different
    // mechanism.
    if htag == crate::layout::TAG_OBJ
        && let Some(r) = unsafe {
            crate::define_struct::struct_receiver_define(
                obj,
                key,
                tag,
                value,
                flags_byte,
                throw_on_refusal,
            )
        }
    {
        return r;
    }
    // Any other non-dynobj cell (a declared struct field, typed Date /
    // RegExp / Map / ...) — same family as the three arms above:
    // walking a foreign layout as a dynobj header (count/cap read off
    // arbitrary field bytes) is silent corruption. These receivers
    // have no expando define storage yet (RFC 20260721 刀 2b backlog —
    // mirrored by the lowering's typed-receiver no-op);
    // release the transferred [[Value]] stake and leave the cell
    // intact. The boolean flavor answers 0 — nothing was defined, and
    // false is the honest spelling (the prop_delete Tag::Obj
    // precedent); the throwing flavor keeps the silent no-op.
    if htag != crate::layout::TAG_DYNOBJ {
        if flags_byte & DEFINE_PRESENT_VALUE != 0 {
            unsafe { drop_rejected_value(tag, value) };
        }
        return if throw_on_refusal { 1 } else { 0 };
    }
    // A define landing on a builtin `<Ctor>.prototype` singleton is
    // a monkey-patch — note it for the fast-arm pre-gate (RFC
    // 20260721 刀 11 G13; one relaxed load when no singleton exists).
    // The gate keys off the method NAME, so a symbol key has nothing
    // to report — and must not have the Str payload offsets read off
    // its cell, which is why the name comes from `key_str_bytes`.
    if let Some((data, len)) = unsafe { key_str_bytes(key) } {
        unsafe { __torajs_builtin_proto_note_own_write(obj, data, len as i64) };
    }
    // Dense-array-full guard — same shape as set.rs.
    if unsafe { entries_len(obj) } == unsafe { entries_cap(obj) } {
        unsafe {
            resize(obj_slot);
            obj = *obj_slot;
        }
    }

    let pr = unsafe { probe(obj, key as *const c_void) };
    let ent = unsafe { entries(obj) };

    let has_value = flags_byte & DEFINE_PRESENT_VALUE != 0;
    let desc_writable = flags_byte & DEFINE_FLAG_WRITABLE != 0;
    let desc_enumerable = flags_byte & DEFINE_FLAG_ENUMERABLE != 0;
    let desc_configurable = flags_byte & DEFINE_FLAG_CONFIGURABLE != 0;

    if pr.found {
        // Existing entry — §10.1.6.3 validate + apply, split out to
        // keep this dispatcher under the 200-line fn hard limit.
        unsafe {
            crate::define_redefine::redefine_entry(
                ent.add(pr.entry as usize),
                tag,
                value,
                flags_byte,
                throw_on_refusal,
            )
        }
    } else {
        // RFC C5b-4 — Object.defineProperty(O, "newKey", desc) on a
        // sealed / non-extensible dict must throw TypeError (wording
        // matches bun for cross-runtime parity). Existing-key redefine
        // is gated by the `cur_configurable` branch above; this
        // catches the "add a new own property" path.
        let header_flags = unsafe { *(obj.cast::<u8>().add(6) as *const u16) };
        if header_flags & DYNOBJ_HDR_FLAG_NON_EXTENSIBLE != 0 {
            return unsafe {
                refuse(
                    throw_on_refusal,
                    c"Attempting to define property on object that is not extensible.".as_ptr()
                        as *const u8,
                    has_value,
                    tag,
                    value,
                )
            };
        }
        // Fresh define: append to the dense array (insertion order).
        // Absent flags default to false (spec §10.1.6.2).
        let mut new_flags: u64 = 0;
        if desc_writable {
            new_flags |= BUCKET_FLAG_WRITABLE;
        }
        if desc_enumerable {
            new_flags |= BUCKET_FLAG_ENUMERABLE;
        }
        if desc_configurable {
            new_flags |= BUCKET_FLAG_CONFIGURABLE;
        }
        let (init_tag, init_value) = if has_value {
            (tag & BUCKET_TAG_MASK, value)
        } else {
            // No .value present — default [[Value]] to undefined.
            (ANY_UNDEF, 0)
        };
        let e_idx = unsafe { entries_len(obj) };
        unsafe {
            __torajs_rc_inc(key);
            *ent.add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, new_flags),
                value_anyv: __torajs_anyv_box_from_pair(init_tag as i64, init_value as i64),
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
        1
    }
}
