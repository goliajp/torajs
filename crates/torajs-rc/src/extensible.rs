//! `Object.preventExtensions` / `Object.isExtensible` substrate.
//!
//! Operates on the universal heap header's [`crate::FLAG_NON_EXTENSIBLE`]
//! bit. Type-agnostic: works on any heap value with the standard
//! [`crate::HeapHeader`] prefix (DynObj / Tag::Obj struct / Arr / …).
//!
//! ## STATIC_LITERAL guard
//!
//! Static-literal blocks (`.rodata` constants, escape-analyzed
//! literals, reified builtin method cells) carry
//! [`crate::FLAG_STATIC_LITERAL`]. The flag serves rc-immortality
//! (refcount traffic no-ops, cycle collector skips them); it is
//! orthogonal to [[Extensible]] in the spec sense. `prevent_extensions`
//! short-circuits on it (writing to `.rodata` would SIGBUS), so an
//! attempt to make a reified method cell non-extensible is silently a
//! no-op — but `is_extensible` answers the standard NON_EXTENSIBLE
//! bit either way, so a fresh reified method cell reads as extensible
//! (spec §7.3.15 default), matching bun for
//! `Object.isExtensible(Set.prototype.difference)`. Callers that want
//! spec-primitive-in-cell semantics (Str/Symbol/BigInt cells reading
//! as non-Object) gate on the heap tag one layer up.
//!
//! ## Sealing
//!
//! `Object.seal(obj)` shares this bit (sealed ⇒ non-extensible) plus
//! a walk over the dynobj entry table to clear each entry's
//! `configurable` flag. That walk lives in `torajs-dynobj`; the
//! flag-level work is here.

use core::ffi::c_void;

use crate::{FLAG_NON_EXTENSIBLE, FLAG_SEALED, FLAG_STATIC_LITERAL, HeapHeader};
// FLAG_STATIC_LITERAL is used by `prevent_extensions` / `seal_mark`
// only to short-circuit rodata writes; it does not participate in
// the extensibility answer (see file-level doc).

#[inline]
unsafe fn header_mut(p: *mut c_void) -> &'static mut HeapHeader {
    unsafe { &mut *(p as *mut HeapHeader) }
}

#[inline]
unsafe fn header(p: *const c_void) -> &'static HeapHeader {
    unsafe { &*(p as *const HeapHeader) }
}

/// `Object.preventExtensions(p)` — set the `FLAG_NON_EXTENSIBLE` bit on
/// `p`'s heap header. Returns `p` (chainable, matches the JS API which
/// returns the passed-in object).
///
/// NULL passes through unchanged (no-op + return NULL). Static-literal
/// blocks pass through without bit-flip (writing to `.rodata` would
/// SIGBUS); they already read as non-extensible via
/// [`__torajs_obj_is_extensible`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_prevent_extensions(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return p;
    }
    // SAFETY: caller's contract is that `p` points at a valid heap
    // block with the universal HeapHeader prefix.
    let h = unsafe { header_mut(p) };
    if h.flags & FLAG_STATIC_LITERAL != 0 {
        return p;
    }
    h.flags |= FLAG_NON_EXTENSIBLE;
    p
}

/// `Object.isExtensible(p)` — `true` iff the `FLAG_NON_EXTENSIBLE` bit
/// is unset. NULL reports `false`. STATIC_LITERAL is orthogonal — a
/// reified builtin method cell (immortal, rodata-adjacent) reads as
/// extensible per spec §7.3.15 default; callers that want primitive
/// (Str/Symbol/BigInt cell) non-Object semantics gate on the heap
/// tag at the anyv layer above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_is_extensible(p: *const c_void) -> bool {
    if p.is_null() {
        return false;
    }
    // SAFETY: same as prevent_extensions.
    let h = unsafe { header(p) };
    (h.flags & FLAG_NON_EXTENSIBLE) == 0
}

/// `Object.seal(p)` header-flag work — set both `FLAG_NON_EXTENSIBLE`
/// and `FLAG_SEALED` in one shot. The per-entry "clear configurable"
/// walk is dispatched separately for DynObj cells (see
/// `torajs-dynobj::__torajs_dynobj_seal_entries`); non-DynObj typed
/// cells (Tag::Obj structs / Arr / Closure / …) carry only the header
/// markers because their property table is the static field layout
/// and has no per-entry configurable bit to flip.
///
/// NULL and static-literal blocks short-circuit per the same SIGBUS
/// constraint that gates `prevent_extensions`. Returns `p` so the
/// caller can chain the AnyValue cell return through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_seal_mark(p: *mut c_void) -> *mut c_void {
    if p.is_null() {
        return p;
    }
    let h = unsafe { header_mut(p) };
    if h.flags & FLAG_STATIC_LITERAL != 0 {
        return p;
    }
    h.flags |= FLAG_NON_EXTENSIBLE | FLAG_SEALED;
    p
}

/// `Object.isSealed(p)`'s typed-cell fast path — has the user explicitly
/// called `Object.seal` on this cell? Used by the AnyValue dispatch to
/// short-circuit before the DynObj entry walk; non-DynObj cells answer
/// `isSealed` purely off this bit (their entry table is the static
/// field layout, prevent-only does NOT seal per spec / bun parity).
///
/// Static-literal blocks report `true` (they were sealed in `.rodata`
/// at allocation time).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_is_sealed_marked(p: *const c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let h = unsafe { header(p) };
    if h.flags & FLAG_STATIC_LITERAL != 0 {
        return true;
    }
    (h.flags & FLAG_SEALED) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FLAG_FROZEN, Tag};

    fn make_header(flags: u16) -> HeapHeader {
        HeapHeader {
            refcount: 1,
            type_tag: Tag::Obj as u16,
            flags,
        }
    }

    #[test]
    fn prevent_sets_bit() {
        let mut h = make_header(0);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        let ret = unsafe { __torajs_obj_prevent_extensions(p) };
        assert_eq!(ret, p);
        assert!(h.flags & FLAG_NON_EXTENSIBLE != 0);
    }

    #[test]
    fn prevent_static_literal_noop() {
        let mut h = make_header(FLAG_STATIC_LITERAL);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_obj_prevent_extensions(p) };
        // bit NOT added — writing to rodata would SIGBUS.
        assert!(h.flags & FLAG_NON_EXTENSIBLE == 0);
    }

    #[test]
    fn prevent_null_passes_through() {
        let ret = unsafe { __torajs_obj_prevent_extensions(core::ptr::null_mut()) };
        assert!(ret.is_null());
    }

    #[test]
    fn is_extensible_default_true() {
        let h = make_header(0);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_extensible(p) });
    }

    #[test]
    fn is_extensible_after_prevent_false() {
        let h = make_header(FLAG_NON_EXTENSIBLE);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(!unsafe { __torajs_obj_is_extensible(p) });
    }

    #[test]
    fn is_extensible_static_literal_alone_true() {
        // STATIC_LITERAL is an rc-immortality flag, orthogonal to
        // [[Extensible]] — a reified builtin method cell (Set.prototype.difference)
        // carries STATIC_LITERAL but reads as extensible per spec §7.3.15.
        let h = make_header(FLAG_STATIC_LITERAL);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_extensible(p) });
    }

    #[test]
    fn is_extensible_static_literal_plus_non_extensible_false() {
        // Callers that explicitly want a rodata-adjacent block to
        // report non-extensible must also set FLAG_NON_EXTENSIBLE
        // (e.g. a mint site that hard-locks a well-known cell).
        let h = make_header(FLAG_STATIC_LITERAL | FLAG_NON_EXTENSIBLE);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(!unsafe { __torajs_obj_is_extensible(p) });
    }

    #[test]
    fn is_extensible_null_false() {
        assert!(!unsafe { __torajs_obj_is_extensible(core::ptr::null()) });
    }

    #[test]
    fn non_extensible_disjoint_from_frozen() {
        // Setting NON_EXTENSIBLE leaves FROZEN untouched and vice
        // versa — they occupy disjoint bits per the layout contract.
        let mut h = make_header(FLAG_FROZEN);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_obj_prevent_extensions(p) };
        assert!(h.flags & FLAG_FROZEN != 0);
        assert!(h.flags & FLAG_NON_EXTENSIBLE != 0);
    }

    #[test]
    fn seal_mark_sets_both_bits() {
        let mut h = make_header(0);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        let ret = unsafe { __torajs_obj_seal_mark(p) };
        assert_eq!(ret, p);
        assert!(h.flags & FLAG_NON_EXTENSIBLE != 0);
        assert!(h.flags & FLAG_SEALED != 0);
    }

    #[test]
    fn is_sealed_marked_reads_bit() {
        let h = make_header(FLAG_SEALED);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_sealed_marked(p) });
    }

    #[test]
    fn is_sealed_marked_static_literal_true() {
        let h = make_header(FLAG_STATIC_LITERAL);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(unsafe { __torajs_obj_is_sealed_marked(p) });
    }

    #[test]
    fn is_sealed_marked_prevent_only_false() {
        // pure preventExtensions sets NON_EXTENSIBLE but not SEALED.
        let h = make_header(FLAG_NON_EXTENSIBLE);
        let p = &h as *const HeapHeader as *const c_void;
        assert!(!unsafe { __torajs_obj_is_sealed_marked(p) });
    }

    #[test]
    fn seal_mark_static_literal_noop() {
        let mut h = make_header(FLAG_STATIC_LITERAL);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_obj_seal_mark(p) };
        // bits NOT added — rodata write would SIGBUS.
        assert!(h.flags & FLAG_NON_EXTENSIBLE == 0);
        assert!(h.flags & FLAG_SEALED == 0);
    }
}
