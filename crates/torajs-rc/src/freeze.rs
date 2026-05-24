//! `Object.freeze` / `Object.isFrozen` substrate — port of
//! `runtime_str.c` L1060-1102.
//!
//! Operates on the universal heap header's `FLAG_FROZEN` bit
//! (defined in [`crate::FLAG_FROZEN`]). Type-agnostic: works on
//! any heap value (Str / Arr / dynobj / Symbol / ...) with the
//! standard `HeapHeader` prefix.
//!
//! ## STATIC_LITERAL guard
//!
//! Static-literal blocks (`.rodata` constants, escape-analyzed
//! literals) carry [`crate::FLAG_STATIC_LITERAL`]. Per ES2015,
//! `Object.freeze(staticLiteral)` is a no-op (the value is already
//! non-extensible). The C runtime also needs this guard because
//! writing to the FROZEN bit on `.rodata` would SIGBUS — we
//! preserve that behavior bit-for-bit.
//!
//! ## Strict-mode throw
//!
//! `__torajs_obj_check_not_frozen` is the mutation guard
//! `ssa_lower` emits at every `obj.field = value` site. If FROZEN
//! is set, it arms a `TypeError` via `__torajs_throw_type_error`
//! (which RETURNS — the actual throw lands at the
//! `emit_throw_check(None)` ssa_lower emits right after); the
//! illegal mutation never executes.

use core::ffi::c_void;

use crate::{FLAG_FROZEN, FLAG_STATIC_LITERAL, HeapHeader};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const u8);
}

#[inline]
unsafe fn header_mut(p: *mut c_void) -> &'static mut HeapHeader {
    unsafe { &mut *(p as *mut HeapHeader) }
}

#[inline]
unsafe fn header(p: *const c_void) -> &'static HeapHeader {
    unsafe { &*(p as *const HeapHeader) }
}

/// `Object.freeze(p)` — set the FROZEN bit on `p`'s heap header.
/// Returns `p` (chainable, matches the JS API which returns the
/// passed-in object).
///
/// NULL passes through unchanged (no-op + return NULL). Static-
/// literal blocks pass through without bit-flip (writing to
/// `.rodata` would SIGBUS).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_freeze(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return p;
    }
    // SAFETY: caller's contract is that `p` points at a valid heap
    // block with the universal HeapHeader prefix.
    let h = unsafe { header_mut(p) };
    if h.flags & FLAG_STATIC_LITERAL != 0 {
        return p;
    }
    h.flags |= FLAG_FROZEN;
    p
}

/// `Object.isFrozen(p)` — read the FROZEN bit. Static-literal
/// blocks report `true` (conceptually immutable `.rodata`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_is_frozen(p: *const c_void) -> bool {
    if p.is_null() {
        return false;
    }
    // SAFETY: same as obj_freeze.
    let h = unsafe { header(p) };
    if h.flags & FLAG_STATIC_LITERAL != 0 {
        return true;
    }
    (h.flags & FLAG_FROZEN) != 0
}

/// Mutation guard emitted at every `obj.field = value` site by
/// `ssa_lower`. If `p`'s FROZEN bit is set, arms a TypeError throw
/// via [`__torajs_throw_type_error`] and returns; ssa_lower's
/// `emit_throw_check(None)` right after diverts control to the
/// user's try/catch BEFORE the field store, so the illegal
/// mutation never happens.
///
/// NULL is treated as "not frozen" — defensive pass-through; the
/// null-deref panic lands elsewhere.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_check_not_frozen(p: *const c_void) {
    if p.is_null() {
        return;
    }
    let h = unsafe { header(p) };
    if h.flags & FLAG_FROZEN != 0 {
        unsafe {
            __torajs_throw_type_error(b"Attempted to assign to readonly property\0".as_ptr())
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tag;

    fn make_header(flags: u16) -> HeapHeader {
        HeapHeader {
            refcount: 1,
            type_tag: Tag::Obj as u16,
            flags,
        }
    }

    #[test]
    fn freeze_sets_bit() {
        let mut h = make_header(0);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        let ret = unsafe { __torajs_obj_freeze(p) };
        assert_eq!(ret, p);
        assert!(h.flags & FLAG_FROZEN != 0);
    }

    #[test]
    fn freeze_static_literal_noop() {
        let mut h = make_header(FLAG_STATIC_LITERAL);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_obj_freeze(p) };
        // FROZEN bit NOT added (static literal is already
        // conceptually frozen + would crash on rodata write).
        assert!(h.flags & FLAG_FROZEN == 0);
    }

    #[test]
    fn freeze_null_passes_through() {
        let ret = unsafe { __torajs_obj_freeze(core::ptr::null_mut()) };
        assert!(ret.is_null());
    }

    #[test]
    fn is_frozen_reads_bit() {
        let h = make_header(FLAG_FROZEN);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_frozen(p) });
    }

    #[test]
    fn is_frozen_static_literal_reports_true() {
        let h = make_header(FLAG_STATIC_LITERAL);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_frozen(p) });
    }

    #[test]
    fn is_frozen_null_reports_false() {
        assert!(!unsafe { __torajs_obj_is_frozen(core::ptr::null()) });
    }
}
