//! Primitive-wrapper `[[Set]]` guards extracted from
//! [`crate::member_set`] as the rotation-196 file-size sweep — the
//! parent had drifted to 505 LOC over the wrapper-set arm history
//! (rotation 185 audit registered it at 505). The two helpers here
//! only reach into the parent's shared const / drop-payload / throw
//! surface, so cluster naturally into a sibling; the main
//! `__torajs_any_member_set` dispatch keeps its arm structure
//! byte-for-byte unchanged.

use core::ffi::c_void;

use crate::member_set::{__torajs_throw_type_error, STR_DATA_OFF, STR_LEN_OFF, drop_payload};

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
