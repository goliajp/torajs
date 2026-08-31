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

use crate::{FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED, FLAG_STATIC_LITERAL, HeapHeader};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_anyv_unbox_tag(v: i64) -> i64;
    fn __torajs_anyv_unbox_value(v: i64) -> i64;
}

#[inline]
unsafe fn header_mut(p: *mut c_void) -> &'static mut HeapHeader {
    unsafe { &mut *(p as *mut HeapHeader) }
}

#[inline]
unsafe fn header(p: *const c_void) -> &'static HeapHeader {
    unsafe { &*(p as *const HeapHeader) }
}

/// `Object.freeze(p)` — spec ES §20.1.2.6 SetIntegrityLevel(O, frozen):
/// mark FLAG_FROZEN + FLAG_SEALED + FLAG_NON_EXTENSIBLE on the heap
/// header (frozen ⇒ sealed ⇒ non-extensible). Returns `p` (chainable).
///
/// Header-only entry point — no dynobj entry table walk. Callers that
/// hold a DynObj cell should route through
/// `torajs_meta::__torajs_anyv_freeze` (RFC 20260716 刀 24) which
/// piggybacks on this + the sibling per-entry writable/configurable
/// clear. This entry stays for typed cells whose SSA source doesn't
/// box (e.g. `Object.freeze(fn)` — a Closure with no dynobj entry
/// table, so the header markers suffice for spec correctness).
///
/// NULL passes through unchanged (no-op + return NULL). Static-
/// literal blocks pass through without bit-flip (writing to
/// `.rodata` would SIGBUS).
///
/// r503 — the one writer of FLAG_FROZEN on a `Tag::Obj` cell, and
/// so a link-guard anchor: the typed field-write guard
/// (`__torajs_obj_check_field_writable`) is stubbed when this atom
/// and the exotic-field writer are both dead. `inline(never)` keeps
/// the atom (and its symbol) whole under the in-crate callers.
#[inline(never)]
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
    h.flags |= FLAG_FROZEN | FLAG_SEALED | FLAG_NON_EXTENSIBLE;
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

/// `Object.isFrozen(any)` — NaN-box-aware entry. ssa_lower routes
/// `Object.isFrozen(arg)` here when `arg` is `Type::Any` (the raw
/// helper above takes `*const c_void` and would deref a NaN-box
/// sentinel as a heap header — see cases#obj-is-frozen-any-segv).
///
/// Per ES2015+ §19.1.2.16: for non-Object input return `true`
/// (frozen by definition); for Object input read the FROZEN bit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_is_frozen_any(v: i64) -> bool {
    const ANY_TAG_HEAP: i64 = 4;
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    if tag != ANY_TAG_HEAP {
        return true;
    }
    // A ShortStr reports Heap too, and unbox_value would MATERIALIZE
    // an rc=1 Str this probe then abandons (546-02 M1 family) — and
    // a string primitive is §20.1.2.14 step-1 frozen by definition
    // (the materialized cell's unset FROZEN bit answered false).
    if !crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return true;
    }
    let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *const c_void;
    // A heap-shaped PRIMITIVE (Str / Symbol / BigInt cell) is step-1
    // frozen too — pre-fix its header's unset FROZEN bit answered
    // false (a static-literal Str happened to answer true through
    // the rodata arm, which is why only the runtime-built shapes
    // showed).
    let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
    if matches!(type_tag, t if t == crate::Tag::Str as u16 || t == crate::Tag::Symbol as u16 || t == crate::Tag::BigInt as u16)
    {
        return true;
    }
    unsafe { __torajs_obj_is_frozen(ptr) }
}

/// `Object.freeze(any)` — NaN-box-aware entry. ssa_lower routes
/// `Object.freeze(arg)` here when `arg` is `Type::Any` (the raw
/// helper above takes `*mut c_void` and would deref a NaN-box
/// sentinel as a heap header — same shape as
/// [`__torajs_obj_is_frozen_any`], see cases#obj-is-frozen-any-segv).
///
/// Per ES2015+ §19.1.2.6 step 1: non-Object input is returned
/// unchanged; Object input gets the FROZEN bit and the same box
/// back.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_freeze_any(v: i64) -> i64 {
    const ANY_TAG_HEAP: i64 = 4;
    let tag = unsafe { __torajs_anyv_unbox_tag(v) };
    // The cell-likeness test keeps a ShortStr out: §19.1.2.6 step 1
    // returns a primitive unchanged, and the old unbox MATERIALIZED
    // an rc=1 Str, stamped FROZEN into its throwaway header, and
    // leaked it (546-02 M1 family).
    if tag == ANY_TAG_HEAP && crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        let ptr = unsafe { __torajs_anyv_unbox_value(v) } as *mut c_void;
        // Heap-shaped primitives return unchanged too (step 1) — a
        // freeze must not stamp flags into a Str / Symbol / BigInt
        // cell's header (mirror of the is_frozen_any arm).
        let type_tag = unsafe { *((ptr as *const u8).add(4) as *const u16) };
        if !(type_tag == crate::Tag::Str as u16
            || type_tag == crate::Tag::Symbol as u16
            || type_tag == crate::Tag::BigInt as u16)
        {
            unsafe { __torajs_obj_freeze(ptr) };
        }
    }
    v
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
            __torajs_throw_type_error(b"Attempted to assign to readonly property.\0".as_ptr())
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
