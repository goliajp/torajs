//! Primitive-wrapper `[[Set]]` guards extracted from
//! [`crate::member_set`] as the rotation-196 file-size sweep — the
//! parent had drifted to 505 LOC over the wrapper-set arm history
//! (rotation 185 audit registered it at 505). The two helpers here
//! only reach into the parent's shared const / drop-payload / throw
//! surface, so cluster naturally into a sibling; the main
//! `__torajs_any_member_set` dispatch keeps its arm structure
//! byte-for-byte unchanged.

use core::ffi::c_void;

use torajs_rc::{FLAG_NON_EXTENSIBLE, Tag};

use crate::member_set::{
    __torajs_dynobj_alloc, __torajs_dynobj_has, __torajs_dynobj_set, __torajs_throw_type_error,
    STR_DATA_OFF, STR_LEN_OFF, drop_payload,
};

/// Number/String/Boolean-wrapper lazy props slot — mirror of
/// `torajs-wrapper::WRAPPER_PROPS_OFF` (RFC 20260716 刀 5, rotation
/// 121). Every wrapper cell layout is `[header:8][value:8][props:8]`.
const MEMBER_SET_WRAPPER_PROPS_OFF: usize = 16;

/// Primitive-wrapper receiver arm (RFC 20260716 刀 5, rotation 121
/// chunk 4) — the String-exotic own-domain refusal, the
/// `[[Extensible]] = false` fresh-key gate, and the lazy `+16`
/// expando props write. Moved out of the parent dispatch (rotation
/// 268 — Reflect.set 参数化前的余量腾挪, mechanical move).
///
/// # Safety
/// `ptr` is a live wrapper cell of `cell_tag`; `key` is a live Str
/// cell; `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn set_wrapper_member(
    ptr: *mut c_void,
    cell_tag: u16,
    key: *mut c_void,
    tag: u64,
    value: u64,
) {
    unsafe {
        // §10.4.3 String Exotic — `"length"` and the in-range
        // code-unit indices are non-writable own properties:
        // [[Set]] answers false and strict assignment turns
        // that into a TypeError (§13.15.2; bun throws). The
        // pre-fix store landed in the expando dynobj, so the
        // dynamic-key read handed back the shadow value and
        // test262's isWritable probe saw a writable length.
        if cell_tag == Tag::StringWrapper as u16 && strwrapper_own_domain_key(ptr, key) {
            drop_payload(tag, value);
            __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            return;
        }
        let props_slot = ptr.cast::<u8>().add(MEMBER_SET_WRAPPER_PROPS_OFF) as *mut u64;
        let mut props = *props_slot as *mut c_void;
        // §10.1.5.1 [[Set]] on a non-extensible wrapper rejects new
        // keys. The wrapper cell owns the `FLAG_NON_EXTENSIBLE`
        // bit (set by `Object.preventExtensions(w)`), not the
        // expando dynobj it lazily allocates — so `dynobj_set`'s
        // own gate can't cover this; check the wrapper header
        // directly here. Update to an existing expando key
        // (`w.foo = 2` after `w.foo = 1; preventExtensions(w)`)
        // stays allowed — matches bun-parity, mirror of the
        // dynobj_set / dynobj_define gates.
        let wrapper_flags = *(ptr.cast::<u8>().add(6) as *const u16);
        if wrapper_flags & FLAG_NON_EXTENSIBLE != 0 {
            let key_present = !props.is_null()
                && __torajs_dynobj_has(props as *const c_void, key as *const c_void) != 0;
            if !key_present {
                reject_non_extensible(tag, value);
                return;
            }
        }
        if props.is_null() {
            props = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_set(&mut props, key, tag, value);
        *props_slot = props as u64;
    }
}

/// True when `key` names a `Tag::StringWrapper` inherent own
/// property — `"length"` or a canonical code-unit index in
/// `[0, len)` (§10.4.3 StringGetOwnProperty). Both are
/// non-writable: the caller refuses the store instead of letting
/// it shadow through the expando dynobj.
pub(crate) unsafe fn strwrapper_own_domain_key(ptr: *mut c_void, key: *const c_void) -> bool {
    unsafe {
        let k = key as *const u8;
        let key_len = (k.add(STR_LEN_OFF) as *const u32).read();
        let bytes = core::slice::from_raw_parts(k.add(STR_DATA_OFF), key_len as usize);
        if bytes == b"length" {
            return true;
        }
        let Some(idx) = crate::member_get::canonical_index(bytes) else {
            return false;
        };
        let inner = (ptr.cast::<u8>().add(8) as *const *const c_void).read();
        let len = if inner.is_null() {
            0
        } else {
            inner.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() as u64
        };
        idx < len
    }
}

/// Wrapper-cell `[[Set]]` rejection when the receiver has
/// `[[Extensible]] = false` and the key is fresh — mirror of
/// `__torajs_dynobj_set`'s new-key gate wording.
pub(crate) unsafe fn reject_non_extensible(tag: u64, value: u64) {
    unsafe {
        drop_payload(tag, value);
        __torajs_throw_type_error(
            c"Attempting to define property on object that is not extensible.".as_ptr(),
        );
    }
}
