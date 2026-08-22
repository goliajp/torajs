//! Boxing a member-kernel key cell back into an AnyValue — the trap
//! signature hands the handler the key as a *value*
//! (RFC 20260823-proxy-substrate 刀 1).
//!
//! The member kernels key by cell pointer (Str or Symbol); a trap
//! takes `(target, key, receiver)`, so the key has to become a
//! first-class value again. Answers OWNED.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

/// Box `key` as an owned AnyValue. A Symbol cell boxes as itself; a
/// Str cell likewise — both are ordinary heap payloads, and the
/// `+1` `__torajs_anyv_box_pointer` does not take is taken here.
///
/// # Safety
/// `key` is a live Str or Symbol cell.
pub(crate) unsafe fn key_to_any(key: *const c_void) -> AnyValue {
    unsafe {
        let p = key as *mut c_void;
        debug_assert!({
            let t = p.cast::<u8>().add(4).cast::<u16>().read();
            t == Tag::Str as u16 || t == Tag::Symbol as u16
        });
        torajs_rc::__torajs_rc_inc(p);
        __torajs_anyv_box_pointer(p)
    }
}
