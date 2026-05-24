//! Cycle-collector layout constants + color helpers + classification
//! predicates.
//!
//! Mirrors `runtime_cycle.c`'s 1:1 — every byte offset, color bit
//! pattern, and `__torajs_class_layouts` extern declaration matches
//! the original C file so ABI-compat is preserved across the port.
//!
//! ## Color bits (flags field of heap header)
//!
//! Two bits at `(flags >> 3) & 3`:
//!
//! ```text
//!   BLACK  = 0   in use, no cycle suspicion
//!   GRAY   = 1   being marked during a current trial-deletion pass
//!   PURPLE = 2   buffered as a potential cycle root
//!   WHITE  = 3   confirmed garbage; freed by collect phase
//! ```
//!
//! `FLAG_BUFFERED` (bit 5) is the de-dup guard for `cycle_buffer` —
//! second push of the same already-in-buffer pointer is a fast-path
//! no-op via a single bit check.
//!
//! ## class_layouts table (codegen-emitted)
//!
//! `ssa_inkwell::compile_module` emits a `__torajs_class_layouts`
//! global (LLVM array of `{ u32 n_children, ptr child_offsets }`) +
//! a `__torajs_n_class_layouts` u32 holding the table length. The
//! cycle collector indexes the table by `class_tag - 1` (read from
//! the obj header's `+8` slot) to find child-pointer field offsets.

use core::ffi::c_void;

/// Universal heap-header tag for class instances. Mirrors
/// `__TORAJS_TAG_OBJ = 1` in runtime_cycle.c.
pub const TAG_OBJ: u16 = 1;

/// Universal heap-header tag for arrays. Mirrors
/// `__TORAJS_TAG_ARR = 2`.
pub const TAG_ARR: u16 = 2;

/// `STATIC_LITERAL` flag bit. Set on heap blocks promoted to
/// data-segment lifetime — cycle collector skips them entirely
/// (immortal, never owned).
pub const FLAG_STATIC_LITERAL: u16 = 4;

/// Color bit-shift inside `flags`. Mirrors `COLOR_SHIFT = 3`.
pub const COLOR_SHIFT: u32 = 3;

/// Mask covering the 2 color bits.
pub const COLOR_MASK: u16 = 3 << COLOR_SHIFT;

pub const COLOR_BLACK: u16 = 0 << COLOR_SHIFT;
pub const COLOR_GRAY: u16 = 1 << COLOR_SHIFT;
pub const COLOR_PURPLE: u16 = 2 << COLOR_SHIFT;
pub const COLOR_WHITE: u16 = 3 << COLOR_SHIFT;

/// "Currently in the cycle root buffer" bit. Guards
/// `cycle_buffer` against double-push and lets `cycle_unbuffer` do
/// a single-bit check before its linear scan.
pub const FLAG_BUFFERED: u16 = 1 << 5;

/// Offset of the `class_tag` u32 inside a class-instance Obj. Lives
/// right after the universal 8-byte heap header.
pub const OBJ_CLASS_TAG_OFF: usize = 8;

/// Universal heap header — 8 bytes, ABI-shared with
/// `torajs_rc::HeapHeader` + every `__torajs_heap_header_t` typedef
/// repeated in the C runtime translation units.
#[repr(C, align(8))]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// Class-layout descriptor. One entry per declared class; indexed by
/// `class_tag - 1`. `child_offsets` is a C array of `u32` byte
/// offsets pointing at refcounted-pointer fields within the class
/// instance.
#[repr(C)]
pub struct ClassLayout {
    pub n_children: u32,
    pub child_offsets: *const u32,
}

// SAFETY: `ClassLayout` carries a raw `*const u32` so the auto-derived
// `Sync` check fails. Two reasons this is sound for our purposes:
//   - In the production (`tr build`) path the table lives in the
//     binary's read-only data segment; the cycle collector is the only
//     reader and the runtime is single-threaded.
//   - In cargo-test the table is an empty stub (n_children = 0,
//     child_offsets = null) — no one ever reads past the head field.
// Cycle collector code must NEVER mutate through `child_offsets`.
unsafe impl Sync for ClassLayout {}

// `__torajs_class_layouts` / `__torajs_n_class_layouts` are emitted
// by ssa_inkwell::compile_module() into every `tr build` user binary.
// At cargo test time they don't exist — we stub them with empty
// definitions so the test binary links. Marking the cfg(test) versions
// `#[no_mangle]` makes them claim the symbol; at `tr build` link the
// real emitted ones take over (these compile to nothing for that
// pathway).
#[cfg(not(test))]
unsafe extern "C" {
    pub static __torajs_class_layouts: ClassLayout;
    pub static __torajs_n_class_layouts: u32;
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub static __torajs_n_class_layouts: u32 = 0;
#[cfg(test)]
#[unsafe(no_mangle)]
pub static __torajs_class_layouts: ClassLayout = ClassLayout {
    n_children: 0,
    child_offsets: core::ptr::null(),
};

#[inline]
pub fn color_of(h: *const HeapHeader) -> u16 {
    unsafe { (*h).flags & COLOR_MASK }
}

#[inline]
pub unsafe fn set_color(h: *mut HeapHeader, color: u16) {
    unsafe { (*h).flags = ((*h).flags & !COLOR_MASK) | color };
}

/// True iff `p` is a declared-class instance with a valid layout in
/// the codegen-emitted table. Filters out: NULL, STATIC_LITERAL,
/// non-OBJ tags, anonymous structs (class_tag = 0), and tags past
/// the table length.
///
/// # Safety
/// `p` must be NULL or a live heap pointer with a valid
/// `HeapHeader` at offset 0. Reading `class_tag` at `+8` is safe
/// only for TAG_OBJ blocks (gated by the type_tag check below).
#[inline]
pub unsafe fn is_class_obj(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let h = p as *const HeapHeader;
    let header = unsafe { &*h };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return false;
    }
    if header.type_tag != TAG_OBJ {
        return false;
    }
    // Read the u32 class_tag at OBJ_CLASS_TAG_OFF (== 8).
    let tag = unsafe { *((p as *const u8).add(OBJ_CLASS_TAG_OFF) as *const u32) };
    if tag == 0 {
        return false; // anonymous struct — no class layout
    }
    // `&raw const` is safe (RFC 2582) on both extern statics (cfg not
    // test) and regular statics (cfg test); the deref is what needs
    // the unsafe block. Unified access keeps `unused_unsafe` happy
    // across both build paths.
    let n_layouts = unsafe { *(&raw const __torajs_n_class_layouts) };
    if tag > n_layouts {
        return false;
    }
    true
}

/// True iff `p` is a non-literal Array whose slots may carry
/// refcounted children. Statically-literal arrays are immortal data,
/// never walked.
///
/// # Safety
/// Same as `is_class_obj` — `p` must be NULL or a live heap pointer
/// with a `HeapHeader` at offset 0.
#[inline]
pub unsafe fn is_visitable_arr(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let h = p as *const HeapHeader;
    let header = unsafe { &*h };
    if header.flags & FLAG_STATIC_LITERAL != 0 {
        return false;
    }
    header.type_tag == TAG_ARR
}

/// True iff any cycle-collector phase can descend into `p`. Today =
/// declared-class instances + arrays.
#[inline]
pub unsafe fn has_walkable_children(p: *mut c_void) -> bool {
    unsafe { is_class_obj(p) || is_visitable_arr(p) }
}

/// Get the `ClassLayout` for a class-instance Obj. Caller must have
/// already verified `is_class_obj(p)` is true.
///
/// # Safety
/// `p` must satisfy `is_class_obj(p)`. Otherwise the read may go
/// past the end of `__torajs_class_layouts`.
#[inline]
pub unsafe fn layout_for_class_obj(p: *mut c_void) -> *const ClassLayout {
    let tag = unsafe { *((p as *const u8).add(OBJ_CLASS_TAG_OFF) as *const u32) };
    // `&raw const` keeps this safe to take across cfg branches; the
    // pointer arithmetic + caller's eventual deref are the unsafe
    // parts (and live in `mark_gray` etc).
    let table: *const ClassLayout = &raw const __torajs_class_layouts;
    unsafe { table.add((tag - 1) as usize) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout() {
        assert_eq!(core::mem::size_of::<HeapHeader>(), 8);
        assert_eq!(core::mem::align_of::<HeapHeader>(), 8);
        assert_eq!(core::mem::offset_of!(HeapHeader, refcount), 0);
        assert_eq!(core::mem::offset_of!(HeapHeader, type_tag), 4);
        assert_eq!(core::mem::offset_of!(HeapHeader, flags), 6);
    }

    #[test]
    fn class_layout_struct() {
        // ClassLayout: u32 + 4B pad + ptr = 16B, align 8 (ptr alignment).
        assert_eq!(core::mem::size_of::<ClassLayout>(), 16);
        assert_eq!(core::mem::align_of::<ClassLayout>(), 8);
    }

    #[test]
    fn color_constants_match_c() {
        // Mirrors runtime_cycle.c — COLOR_SHIFT = 3, mask = 3 << 3 = 0x18.
        assert_eq!(COLOR_SHIFT, 3);
        assert_eq!(COLOR_MASK, 0x18);
        assert_eq!(COLOR_BLACK, 0x00);
        assert_eq!(COLOR_GRAY, 0x08);
        assert_eq!(COLOR_PURPLE, 0x10);
        assert_eq!(COLOR_WHITE, 0x18);
        assert_eq!(FLAG_BUFFERED, 0x20);
        assert_eq!(FLAG_STATIC_LITERAL, 4);
    }

    #[test]
    fn color_round_trip() {
        let mut h = HeapHeader {
            refcount: 1,
            type_tag: TAG_OBJ,
            flags: 0,
        };
        unsafe { set_color(&mut h, COLOR_PURPLE) };
        assert_eq!(color_of(&h), COLOR_PURPLE);
        unsafe { set_color(&mut h, COLOR_GRAY) };
        assert_eq!(color_of(&h), COLOR_GRAY);
        // Other flag bits preserved across color writes:
        h.flags |= FLAG_BUFFERED;
        unsafe { set_color(&mut h, COLOR_WHITE) };
        assert_eq!(color_of(&h), COLOR_WHITE);
        assert_ne!(h.flags & FLAG_BUFFERED, 0);
    }
}
