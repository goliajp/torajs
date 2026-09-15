//! The `Tag::Obj` arm of the any-lane member set — a class instance
//! (nominal struct) receiver: own accessor, layout data field, then
//! the `+24` expando dict, with the class prototype chain consulted
//! before an expando is CREATED (§10.1.9 OrdinarySet). Moved out of
//! `member_set.rs` (rotation 653: that file sat at the 500-line cap
//! and the declared-field shadow store, RFC 20260915-declared-field-
//! shadow, lands in this arm).

use core::ffi::c_void;

use crate::member_set::{
    __torajs_dynobj_alloc, __torajs_dynobj_has, __torajs_dynobj_set, drop_payload,
};
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use torajs_rc::AnySlotTag;

unsafe extern "C" {
    /// torajs-dynobj — the declared-field shadow store / supersede
    /// (RFC 20260915-declared-field-shadow).
    fn __torajs_obj_declared_field_shadow_set(
        obj: *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    fn __torajs_obj_declared_field_shadow_clear(obj: *mut c_void, key: *mut c_void);
    /// torajs-dynobj — the field's live W/E/C bits, `-1` when the
    /// layout declares nothing under `key`.
    fn __torajs_obj_declared_field_attrs(obj: *mut c_void, key: *mut c_void) -> i64;
}

/// `torajs_dynobj::layout::BUCKET_FLAG_WRITABLE` mirror.
const BUCKET_FLAG_WRITABLE: u64 = 1 << 0;

/// The `Tag::Obj` (class instance / nominal struct) arm of
/// [`crate::member_set::any_member_set_impl`] — extracted so the dispatcher stays under
/// the function cap when the Proxy arm went in.
///
/// The order is §10.1.9 OrdinarySet's: an own accessor, then a
/// layout DATA field, then the +24 expando dict — and before an
/// expando is CREATED, the class prototype chain gets its say, so a
/// reified accessor up there writes through its setter instead of
/// being shadowed by a fresh own key.
///
/// `None` means "this arm did not claim the write" (the layout does
/// carry the name, or an own accessor is present but get-only) and
/// the caller falls through to its loud reject.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell that `recv` boxes; `key` a live
/// Str cell; the `(tag, value)` pair transfers in.
pub(crate) unsafe fn set_obj_member(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let k = torajs_rc::str_wtf8::StrWtf8::of(key);
        let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        if crate::struct_probe::__torajs_struct_accessor_set(ptr, k.as_ptr(), k.len(), value_anyv) {
            // The setter borrowed the value out of argv; the write
            // path owns the payload it was handed.
            drop_payload(tag, value);
            return Some(1);
        }
        // RFC 20260718-error-message-own-prop — a class-layout
        // DATA field takes the store when the payload matches the
        // slot's type (Any / Str / I64 / F64 / Bool; frozen gate
        // inside). A mismatch falls through to the loud reject —
        // a typed slot never silently coerces.
        // RFC 20260915 — a deleted declared field revives (or refuses).
        if let Some(r) =
            crate::struct_probe::deleted_field_write_gate(ptr, key, tag, value, throw_on_refusal)
        {
            return Some(r);
        }
        if crate::struct_field_ffi::frozen_data_field_refuses(
            ptr,
            key,
            tag,
            value,
            throw_on_refusal,
        ) {
            return Some(0);
        }
        // A sidecar that demoted the field to non-writable refuses
        // the any-lane store — matching payload or not (§10.1.9.2
        // step 2.a). Only an instance something redefined carries
        // the header bit, so an untouched one skips the probe.
        if crate::member_get_layout::header_flag(ptr, torajs_rc::FLAG_OBJ_EXOTIC_FIELD) {
            let attrs = __torajs_obj_declared_field_attrs(ptr, key);
            if attrs >= 0 && attrs as u64 & BUCKET_FLAG_WRITABLE == 0 {
                drop_payload(tag, value);
                if throw_on_refusal {
                    crate::member_set::__torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr(),
                    );
                }
                return Some(0);
            }
        }
        if crate::struct_error_msg::struct_data_field_set(ptr, key, tag, value) {
            // The slot took it; a shadow an earlier mismatch left
            // behind is superseded (free when there is none).
            __torajs_obj_declared_field_shadow_clear(ptr, key);
            return Some(1);
        }
        // RFC 20260915-declared-field-shadow — a declared DATA field
        // whose slot cannot hold the payload keeps the value in its
        // sidecar; the own faces read it before the slot. The
        // accessor / frozen / non-writable gates above already had
        // their say. `undefined` is spelled as the undef cell so the
        // word stays distinct from the attribute-only filler.
        if crate::struct_probe::struct_layout_declares(ptr, key) {
            let (t, v) = if tag == AnySlotTag::Undef as u64 {
                (
                    AnySlotTag::Heap as u64,
                    torajs_rc::undef_cell::undef_cell_ptr() as u64,
                )
            } else {
                (tag, value)
            };
            __torajs_obj_declared_field_shadow_set(ptr, key, t, v);
            return Some(1);
        }
        // RFC 20260714-struct-dynamic-props blade 2 — a name the
        // layout doesn't carry lands in the +24 expando dynobj
        // (lazily allocated; blade 1 zeroed the slot at every
        // alloc site). §10.1.5.1 [[Set]] on a non-extensible
        // struct rejects NEW keys — the struct header owns the
        // flag, not the expando dict, so the gate sits here
        // (wrapper-arm mirror); an update to a live expando key
        // stays allowed. dynobj_set consumes the caller's +1 and
        // writes the possibly-relocated block back through the
        // slot.
        if crate::struct_probe::struct_field_pair(ptr, key).is_none()
            && !crate::struct_probe::struct_accessor_key(ptr, key)
        {
            let props_slot = ptr
                .cast::<u8>()
                .add(crate::member_get_layout::OBJ_PROPS_OFF)
                as *mut *mut c_void;
            // Rotation 441 (3c) — §10.1.9.2: an own miss (layout,
            // own accessor, expando dict all silent) consults the
            // prototype chain before the own create — a
            // runtime-computed accessor reified on `__proto_<C>`
            // writes through its setter with this receiver (or
            // refuses get-only), instead of minting an own expando
            // entry that would shadow the getter on every later
            // read. An own expando key stays an ordinary own
            // write — OrdinarySet never consults the chain for it.
            let own_expando = !(*props_slot).is_null()
                && __torajs_dynobj_has(*props_slot, key as *const c_void) != 0;
            if !own_expando
                && let Some(r) = crate::member_set_dynobj::inherited_set_handled(
                    ptr,
                    recv,
                    key,
                    tag,
                    value,
                    throw_on_refusal,
                )
            {
                return Some(r);
            }
            if crate::member_set_closure::refuse_new_key_non_extensible(
                ptr,
                own_expando,
                tag,
                value,
                throw_on_refusal,
            ) {
                return Some(0);
            }
            if (*props_slot).is_null() {
                // r502 — the first attach goes through the rc entry
                // the instance's drop seams are guarded on.
                torajs_rc::obj_entry::__torajs_obj_props_attach(
                    ptr.cast::<u8>(),
                    __torajs_dynobj_alloc(),
                );
            }
            __torajs_dynobj_set(props_slot, key, tag, value);
            // The instance now owns a key its layout never
            // mentions, so a compile-time member list is no longer
            // the whole story — the fact every unfolding reader
            // gates on.
            let flags = ptr.cast::<u8>().add(6) as *mut u16;
            *flags |= torajs_rc::FLAG_OBJ_EXPANDO;
            return Some(1);
        }
        None
    }
}
