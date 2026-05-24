//! `WeakRef` heap-block layout + observer-kind constants.
//!
//! Mirrors `runtime_weakref.c`'s layout 1:1 (same separately-compiled
//! ABI-shared pattern torajs-arr / torajs-dynobj / torajs-collections
//! use). `#[repr(C, align(8))]` on the structs guarantees ABI-compat
//! with the existing C-side definitions (still present in
//! runtime_weakref.c during the progressive P4.3' port; collapse at
//! P4.3'-e closer).
//!
//! ```text
//! WeakRef struct (16 bytes, 8-byte aligned):
//!   offset 0 : universal heap header (8B; refcount + type_tag + flags)
//!   offset 8 : target (*mut c_void) — observed pointer; NULL = reclaimed
//! ```

use core::ffi::c_void;

/// `type_tag` value for WeakRef heap blocks (matches
/// `runtime_weakref.c::__TORAJS_TAG_WEAKREF` = 11 + the
/// `runtime_str.c::value_drop_heap` dispatch case).
pub const TAG_WEAKREF: u16 = 11;

/// `type_tag` value for WeakMap heap blocks (matches the
/// `runtime_str.c::value_drop_heap` dispatch case `__TORAJS_TAG_WEAKMAP
/// = 12`). Distinct from TAG_WEAKREF; dispatch routes to
/// `__torajs_weakmap_drop`.
pub const TAG_WEAKMAP: u16 = 12;

/// `type_tag` value for WeakSet heap blocks (matches
/// `__TORAJS_TAG_WEAKSET = 13`). Distinct shape from WeakMap (no
/// value side), but value_drop_heap dispatch is symmetrical.
pub const TAG_WEAKSET: u16 = 13;

/// `STATIC_LITERAL` flag bit (mirrors `torajs_rc::FLAG_STATIC_LITERAL`).
/// Set on heap blocks promoted to data-segment lifetime — drop must
/// be a no-op when this bit is set. WeakRef literals don't exist
/// in practice but the bit-check mirrors the original C body for
/// completeness, since static-promotion of any heap block is a
/// runtime-wide invariant.
pub const FLAG_STATIC_LITERAL: u16 = 4;

/// Observer-kind discriminant for the shared registry. Matches
/// `runtime_weakref.c::ObserverKind` numeric values. Each new
/// observer kind (WeakMap / WeakSet later) appends another u32.
/// Kept as `u32` (not Rust enum) because C-side `ObserverKind` is
/// emitted to the registry as a raw int; bridging through enum
/// adds zero safety here and would force a transmute on every
/// register/deregister call site.
pub const OBSERVER_WEAKREF: u32 = 0;
#[allow(dead_code)] // P4.3'-c will start using this.
pub const OBSERVER_WEAKMAP: u32 = 1;
#[allow(dead_code)] // P4.3'-d will start using this.
pub const OBSERVER_WEAKSET: u32 = 2;

/// In-block heap header — 8 bytes, ABI-shared with
/// `torajs_rc::HeapHeader` + the C-side `__torajs_heap_header_t`.
#[repr(C, align(8))]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// `WeakRef` — 16-byte struct: header + observed target pointer.
/// `target` is set on construction; cleared to NULL by
/// `runtime_weakref.c::__torajs_weakref_target_dying` when the target's
/// strong rc transitions to zero. Reads (via `deref`) never bump the
/// observed reference — the registry side observes the death event.
#[repr(C)]
pub struct WeakRef {
    pub header: HeapHeader,
    pub target: *mut c_void,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_c() {
        // HeapHeader: u32 + u16 + u16 = 8B, align 8.
        assert_eq!(core::mem::size_of::<HeapHeader>(), 8);
        assert_eq!(core::mem::align_of::<HeapHeader>(), 8);

        // WeakRef: header(8) + ptr(8) = 16B, align 8.
        assert_eq!(core::mem::size_of::<WeakRef>(), 16);
        assert_eq!(core::mem::align_of::<WeakRef>(), 8);

        // Field offsets must match runtime_weakref.c::WeakRef.
        assert_eq!(core::mem::offset_of!(WeakRef, header), 0);
        assert_eq!(core::mem::offset_of!(WeakRef, target), 8);

        // HeapHeader field offsets must match
        // runtime_weakref.c::__torajs_heap_header_t.
        assert_eq!(core::mem::offset_of!(HeapHeader, refcount), 0);
        assert_eq!(core::mem::offset_of!(HeapHeader, type_tag), 4);
        assert_eq!(core::mem::offset_of!(HeapHeader, flags), 6);
    }

    #[test]
    fn observer_kinds_disjoint_and_match_c() {
        // ObserverKind enum in runtime_weakref.c: WEAKREF=0, WEAKMAP=1,
        // WEAKSET=2. Drift here = silent registry mis-dispatch.
        assert_eq!(OBSERVER_WEAKREF, 0);
        assert_eq!(OBSERVER_WEAKMAP, 1);
        assert_eq!(OBSERVER_WEAKSET, 2);
    }

    #[test]
    fn static_literal_flag_bit() {
        // Mirrors the magic `4` in runtime_weakref.c::__torajs_weakref_drop.
        assert_eq!(FLAG_STATIC_LITERAL, 4);
    }
}
