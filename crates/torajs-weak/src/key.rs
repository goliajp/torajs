//! Weak-key classification + extraction (RC-4 F2).
//!
//! ES §24.3/§24.4 `CanBeHeldWeakly`: only objects (and non-registered
//! symbols) are legal WeakMap/WeakSet keys. In torajs terms a legal
//! key is a heap cell whose `HeapHeader::type_tag` is object-like —
//! Str and BigInt heap cells back JS *primitives* and must be
//! rejected exactly like a bare number.
//!
//! Two extraction shapes, shared by both lanes (the any-receiver
//! dispatcher in torajs-anyvalue and the typed lowering in
//! torajs-core):
//!
//! - [`__torajs_weak_key_from_any`] — NULL for an illegal key; the
//!   kernels treat a NULL key as absent (`has`/`delete` → false,
//!   `get` → undefined) per spec steps 24.3.3.x.4.
//! - [`__torajs_weak_key_from_any_or_throw`] — records a pending
//!   TypeError ("Invalid value used as weak map key") for an illegal
//!   key and returns NULL; the kernels no-op on a NULL key and the
//!   caller's throw-check propagates (torajs-throw convention — the
//!   throw extern returns normally, control flow continues).

use core::ffi::c_void;

use crate::layout::HeapHeader;

/// NaN-box mirrors — must match `torajs_anyvalue::nanbox` (which
/// locks its values by unit test). Repeated here so torajs-weak
/// stays dependency-tight, same convention as torajs-rc's
/// `nan_box_is_cell_like`.
pub(crate) const AV_TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
pub(crate) const AV_TAG_BIT_TYPE_OTHER: u64 = 0x0000_0000_0000_0002;
pub(crate) const AV_UNDEFINED: u64 = 0x0000_0000_0000_000A;

/// `HeapHeader::type_tag` mirrors of `torajs_rc::Tag` — only the two
/// primitive-backing tags a weak key must reject.
const TAG_STR: u16 = 0;
const TAG_BIGINT: u16 = 10;

unsafe extern "C" {
    /// torajs-throw — records the pending throw in TLS and returns;
    /// the SSA-side caller's `emit_throw_check` propagates.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `true` iff the AnyValue bit pattern is a heap cell (its bits ARE
/// the pointer). Mirrors `torajs_anyvalue::nanbox::is_cell`.
#[inline]
pub(crate) const fn av_is_cell(v: u64) -> bool {
    (v & AV_TOP_16_MASK) == 0 && (v & AV_TAG_BIT_TYPE_OTHER) == 0 && v != 0
}

/// Classify + extract: the key ptr when `av` is a legal weak key,
/// NULL otherwise.
#[inline]
fn valid_weak_key(av: u64) -> *mut c_void {
    if !av_is_cell(av) {
        return core::ptr::null_mut();
    }
    let h = av as usize as *const HeapHeader;
    // SAFETY: cell bit-pattern ⇒ av is a live heap block ptr per the
    // NaN-box contract; the header is the first 8 bytes.
    let tag = unsafe { (*h).type_tag };
    if tag == TAG_STR || tag == TAG_BIGINT {
        return core::ptr::null_mut();
    }
    av as usize as *mut c_void
}

/// Extraction for `has` / `get` / `delete` — illegal key reads as
/// absent (NULL), never throws.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weak_key_from_any(av: u64) -> *mut c_void {
    valid_weak_key(av)
}

/// Extraction for `set` / `add` — illegal key records a pending
/// TypeError and returns NULL (kernels no-op on NULL key; the
/// caller's throw-check propagates).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weak_key_from_any_or_throw(av: u64) -> *mut c_void {
    let p = valid_weak_key(av);
    if p.is_null() {
        unsafe { __torajs_throw_type_error(c"Invalid value used as weak map key".as_ptr()) };
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn av_mirror_constants_locked() {
        // Mirrors of `torajs_anyvalue::nanbox` — must stay in sync
        // (the source crate locks its own values by unit test too).
        assert_eq!(AV_UNDEFINED, 0x0A);
        assert!(av_is_cell(0x6000_0000_1230)); // aligned heap ptr bits
        assert!(!av_is_cell(0)); // NULL
        assert!(!av_is_cell(0x02)); // JS null sentinel
        assert!(!av_is_cell(0x0A)); // undefined sentinel
        assert!(!av_is_cell(0xFFFE_0000_0000_0001)); // boxed int32
    }

    #[test]
    fn primitive_backing_tags_rejected() {
        let str_cell = HeapHeader {
            refcount: 1,
            type_tag: TAG_STR,
            flags: 0,
        };
        let obj_cell = HeapHeader {
            refcount: 1,
            type_tag: 1, // Tag::Obj
            flags: 0,
        };
        let str_av = &str_cell as *const HeapHeader as u64;
        let obj_av = &obj_cell as *const HeapHeader as u64;
        assert!(
            valid_weak_key(str_av).is_null(),
            "Str cell is a JS primitive"
        );
        assert_eq!(valid_weak_key(obj_av), obj_av as usize as *mut c_void);
        assert!(valid_weak_key(0x0A).is_null(), "undefined sentinel");
        assert!(valid_weak_key(0xFFFE_0000_0000_0005).is_null(), "boxed int");
    }
}
