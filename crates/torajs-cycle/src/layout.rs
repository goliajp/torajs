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
//! torajs-link emits a `__torajs_class_layouts`
//! global (array of `{ u32 n_children, ptr child_offsets }`) +
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

/// `torajs_rc::Tag::DynObj = 14` — the compact insertion-ordered
/// dict (RFC 20260717-cycle-walk-dynobj blade 2).
pub const TAG_DYNOBJ: u16 = 14;

/// Primitive wrapper tags (`torajs_rc::Tag::{Number,String,Boolean}
/// Wrapper` — RFC 20260717-cycle-walk-dynobj blade 3). Each carries
/// a lazy expando props-dynobj pointer at
/// [`WRAPPER_PROPS_OFF`], the wrapper's only walkable child.
pub const TAG_NUMBER_WRAPPER: u16 = 21;
/// See [`TAG_NUMBER_WRAPPER`].
pub const TAG_STRING_WRAPPER: u16 = 22;
/// See [`TAG_NUMBER_WRAPPER`].
pub const TAG_BOOLEAN_WRAPPER: u16 = 23;
/// See [`TAG_NUMBER_WRAPPER`]. `Object(Symbol())` carries an expando
/// bag at the same offset its three siblings do and an owning `+1`
/// on the inner Symbol cell, exactly like a StringWrapper.
pub const TAG_SYMBOL_WRAPPER: u16 = 24;

/// `Tag::Closure = 3` — env-first closure cell (RFC 20260717
/// closure-env-cycle knife 3). Layout mirror of torajs-core
/// `ssa_lower.rs` CLOSURE_* offsets: `{ hdr | fn_ptr@8 | drop_fn@16 |
/// props@24 | boxed_entry@32 | trace_fn@40 | caps@48+ }`.
pub const TAG_CLOSURE: u16 = 3;
/// The synthesized `__env_drop_<fn>` pointer — collect_white's
/// closure teardown delegates to it (every release helper it calls
/// is NULL/NaN gated, so cleared cycle edges no-op).
pub const CLOSURE_DROP_FN_OFF: usize = 16;
/// Lazy `f.x = v` expando dynobj — the one child every closure
/// shares at a fixed offset; walked directly, never via trace_fn.
pub const CLOSURE_PROPS_OFF: usize = 24;
/// The synthesized `__env_trace_<fn>` / hand-written `bound_trace`
/// pointer (knife 2). 0 = no capture slot can sit on a cycle.
pub const CLOSURE_TRACE_FN_OFF: usize = 40;

/// Wrapper expando props-dynobj slot offset (torajs-wrapper
/// `WRAPPER_PROPS_OFF` mirror).
pub const WRAPPER_PROPS_OFF: usize = 16;

/// StringWrapper `[[StringData]]` inner Str-cell slot offset
/// (torajs-wrapper `STRING_WRAPPER_CELL_OFF` mirror) — dropped by
/// collect_white's wrapper teardown, never walked (a Str has no
/// children).
pub const STRING_WRAPPER_CELL_OFF: usize = 8;

/// `STATIC_LITERAL` flag bit. Set on heap blocks promoted to
/// data-segment lifetime — cycle collector skips them entirely
/// (immortal, never owned).
pub const FLAG_STATIC_LITERAL: u16 = 4;

/// Color bit-shift inside `flags`. Mirrors torajs-rc
/// `COLOR_SHIFT = 13` (moved off bits 3-4 by RFC 20260706 chunk
/// 573 — the old span overlapped FLAG_ARR_ANY / FLAG_FROZEN, and
/// scan-black cleared a frozen obj's freeze marker).
pub const COLOR_SHIFT: u32 = 13;

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

/// Inline props-dynobj slot of a `Tag::Obj` struct cell (RFC
/// 20260714-struct-dynamic-props blade 1; mirror of
/// `ssa_lower::OBJ_PROPS_OFF`). Same +24 convention as
/// [`CLOSURE_PROPS_OFF`] and the Arr props slot. NULL until the
/// first dynamic write through the `any` lane.
pub const OBJ_PROPS_OFF: usize = 24;

/// `Array<Any>` marker (torajs-rc `FLAG_ARR_ANY` mirror) — 8-byte
/// NaN-box `AnyValue` slots, immediates mixed with cell pointers.
/// Walkable since RFC 20260706 Phase C (chunk 574): `arr_child_at`
/// filters immediates per slot; the historical color-bit overlap is
/// gone (color moved to bits 13-14, chunk 573).
pub const FLAG_ARR_ANY: u16 = 1 << 3;

/// Shift/mask of the 3-bit elem-kind field (torajs-rc `arr_kind`
/// mirror, bits 10-12 of `HeapHeader.flags`).
pub const ARR_ELEM_KIND_SHIFT: u16 = 10;
/// Mask companion of [`ARR_ELEM_KIND_SHIFT`].
pub const ARR_ELEM_KIND_MASK: u16 = 0b111 << ARR_ELEM_KIND_SHIFT;
/// Heap-pointer element slots — the only kind whose 8-byte slots are
/// guaranteed cell pointers the walk may dereference.
pub const ARR_KIND_HEAP: u16 = 4;

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
///
/// W-J Phase A2 (RFC 20260614-w-j-struct-reflect §3 A2): added the
/// `field_metadata_ptr` slot reserved for the field-name + offset +
/// type-tag table that the reflection consumers (Phase B `gOPD` struct
/// arm / Phase C `Object.keys`/`values`/`entries` / Phase D
/// `inspect.rs` Tag::Obj walker) will populate. A2 ships the slot as
/// a NULL stub — no inner field-metadata rodata is emitted yet, and
/// no new chain-fixup rebase target is added (the slot stays as a raw
/// NULL u64 in the on-disk image). Phase A3 will fill it.
///
/// Entry layout: `{ u32 n_children; u32 flags; *const u32 child_offsets;
/// *const c_void field_metadata_ptr; *const c_void method_table_ptr }`
/// — 32 bytes, 8-aligned (matches the LLVM struct rule for
/// `{ i32, i32, ptr, ptr, ptr }`). L3b #4 turned the former pad at +4
/// into a flags word (bit 0 = [`CLASS_LAYOUT_FLAG_NAMED`]); 刀 4
/// (RFC 20260714-t262-top-clusters) extended 24 → 32 with the
/// class-methods dispatch table pointer (read by torajs-structmeta,
/// never by the cycle collector).
#[repr(C)]
pub struct ClassLayout {
    pub n_children: u32,
    /// L3b #4 — bit 0 = declared class (vs anonymous struct shape).
    /// Emit-side mirror: `torajs-link`'s `ENTRY_FLAG_NAMED_CLASS`.
    pub flags: u32,
    pub child_offsets: *const u32,
    /// W-J Phase A2 reserved slot — NULL until A3 fills it.
    pub field_metadata_ptr: *const c_void,
    /// 刀 4 — `.__class_methods_<i>` inner global (NULL when the
    /// class has no runtime-dispatchable methods).
    pub method_table_ptr: *const c_void,
}

/// L3b #4 — [`ClassLayout::flags`] bit 0: entry describes a declared
/// class. The runtime Obj drop buffers only named-class instances as
/// cycle roots (the lower-emitted anon-struct drop skips the buffer
/// scrub for speed — a runtime-buffered anon struct would dangle).
pub const CLASS_LAYOUT_FLAG_NAMED: u32 = 1;

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
// by torajs-link into every `tr build` user binary.
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
    flags: 0,
    child_offsets: core::ptr::null(),
    field_metadata_ptr: core::ptr::null(),
    method_table_ptr: core::ptr::null(),
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
    if header.type_tag != TAG_ARR {
        return false;
    }
    if arr_elems_walkable(header) {
        return true;
    }
    // RFC 20260717 blade 3 — a scalar-kind array can still cycle
    // through its +24 expando props dict (`xs.d = d; d.xs = xs`);
    // the element slots stay off-limits (for_each_child re-checks
    // `arr_elems_walkable`), only the expando slot is enumerated.
    let props = unsafe { *((p as *const u8).add(crate::arr::ARR_PROPS_OFF) as *const *mut c_void) };
    !props.is_null()
}

/// True when the array's ELEMENT slots may carry refcounted children
/// (split out of [`is_visitable_arr`] so `for_each_child` can gate
/// element enumeration independently of the expando slot).
///
/// - **Arr<Any>** (RFC 20260706 Phase C, chunk 574): 8-byte NaN-box
///   slots; `arr_child_at`'s cell-like gate filters immediates.
/// - **ARR_KIND_HEAP**: slots are guaranteed cell pointers. Scalar
///   kinds (raw i64/f64/bool) have no children and their slot bits
///   would deref as garbage; UNSET means the array never crossed a
///   marking boundary — conservatively a leaf (a missed descent
///   under-collects a cycle, never corrupts — L3b #16 residual).
#[inline]
pub fn arr_elems_walkable(header: &HeapHeader) -> bool {
    if header.flags & FLAG_ARR_ANY != 0 {
        return true;
    }
    (header.flags & ARR_ELEM_KIND_MASK) >> ARR_ELEM_KIND_SHIFT == ARR_KIND_HEAP
}

/// True when `p` is a primitive wrapper (Number / String / Boolean /
/// Symbol) that has something to walk: the +16 expando props dict
/// (RFC 20260717 blade 3), its only walkable child. The inner Str or
/// Symbol cell is a leaf the teardown owns, never a walk target.
///
/// Reading that slot rather than answering on the tag alone is the
/// same honesty `is_visitable_arr`, `is_visitable_closure` and
/// `is_visitable_bag` already carry. Almost no wrapper ever grows an
/// expando — `new Number(1)` is the whole shape — so a tag-only
/// answer sent every rc-survivor of the four wrapper tags through
/// `cycle_buffer`: PURPLE, a buffer slot, a mark/scan walk that
/// enumerates nothing, and an unbuffer, all to learn it had no
/// children, plus its share of the 1024-candidate auto-collect
/// threshold it helped reach. Like those three, the answer can flip
/// to false once a corpse's slot is cleared; the `rc > 0` gates in
/// `collect_white`'s second sweep and `defer`'s pass A are what make
/// that safe.
#[inline]
pub unsafe fn is_visitable_wrapper(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0
        || !matches!(
            header.type_tag,
            TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER | TAG_SYMBOL_WRAPPER
        )
    {
        return false;
    }
    let props = unsafe { *((p as *const u8).add(WRAPPER_PROPS_OFF) as *const *mut c_void) };
    !props.is_null()
}

/// True when `p` is a closure env cell with potential walkable
/// children: a non-0 trace_fn (some capture can sit on a cycle,
/// knife 2's construction-site decision) or a live expando props
/// dict. Immortal interned method cells carry FLAG_STATIC_LITERAL —
/// skipped wholesale (CPython-immortal shape), their rc traffic
/// no-ops so they can never be cycle members.
#[inline]
pub unsafe fn is_visitable_closure(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    if header.flags & FLAG_STATIC_LITERAL != 0 || header.type_tag != TAG_CLOSURE {
        return false;
    }
    let trace = unsafe { *((p as *const u8).add(CLOSURE_TRACE_FN_OFF) as *const u64) };
    if trace != 0 {
        return true;
    }
    let props = unsafe { *((p as *const u8).add(CLOSURE_PROPS_OFF) as *const *mut c_void) };
    !props.is_null()
}

/// True when `p`'s bit pattern looks like a real heap pointer (top 16
/// bits zero, low tag bit clear). A NaN-box immediate — an Int32 / f64
/// (top tag bits set) or a Null / Undef / Bool sentinel (low
/// TAG_BIT_TYPE_OTHER bit set) — has no heap header behind it, so
/// dereferencing one is a wild read. Mirrors the identical gate in
/// `torajs-rc::ffi` (rc_inc / rc_dec) and `torajs-value-drop`, kept
/// bit-local so the collector takes no dependency on torajs-anyvalue.
#[inline]
pub(crate) fn nan_box_is_cell_like(p: *mut c_void) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let v = p as u64;
    v != 0 && (v & TOP_16_MASK) == 0 && (v & TAG_BIT_TYPE_OTHER) == 0
}

/// True when `p` is a DynObj dict eligible for the trial-deletion
/// walk (RFC 20260717-cycle-walk-dynobj blade 2). Entry values are
/// NaN-boxes; `dynobj_child_at`'s cell-like gate filters immediates
/// per slot, mirroring the Arr<Any> shape.
#[inline]
pub unsafe fn is_visitable_dynobj(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let header = unsafe { &*(p as *const HeapHeader) };
    header.flags & FLAG_STATIC_LITERAL == 0 && header.type_tag == TAG_DYNOBJ
}

/// True iff any cycle-collector phase can descend into `p`. Today =
/// declared-class instances + arrays + dynobj dicts + closures +
/// primitive wrappers + the [`bag_only_props_off`] shapes.
///
/// The cell-like gate is load-bearing, not defensive: an `any`-typed
/// class field is a cycle child (`Type::Any` is refcounted, so the
/// emit side records its offset in `child_offsets`), but the slot
/// holds a NaN-BOX, not a raw pointer. Every other child consumer
/// already gates — `rc_inc` / `rc_dec` / `__torajs_value_drop_heap`
/// all skip non-cell-like bits — and the collector was the one walk
/// that read the slot raw: `class K { v: any }` with `this.v = 1`
/// stored `0xFFFE…0001`, which the phases then dereferenced as a
/// header (wild read) and decremented (wild write). It only ever
/// fired once a program crossed the 1024-candidate auto-collect
/// threshold, so it read as a mysterious SIGSEGV at scale.
#[inline]
pub unsafe fn has_walkable_children(p: *mut c_void) -> bool {
    if !nan_box_is_cell_like(p) {
        return false;
    }
    unsafe {
        is_class_obj(p)
            || is_visitable_arr(p)
            || is_visitable_dynobj(p)
            || is_visitable_wrapper(p)
            || is_visitable_closure(p)
            || crate::layout_bag::is_visitable_bag(p)
            || crate::proxy::is_visitable_proxy(p)
    }
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
    fn nan_box_immediates_are_not_cell_like() {
        // Int32 / f64 immediates carry the NaN tag in the top bits.
        assert!(!nan_box_is_cell_like(
            0xFFFE_0000_0000_0001u64 as *mut c_void
        ));
        assert!(!nan_box_is_cell_like(
            0xFFFF_0000_0000_0000u64 as *mut c_void
        ));
        // Null / Undef / Bool sentinels set the low type-other bit.
        assert!(!nan_box_is_cell_like(0x02u64 as *mut c_void));
        assert!(!nan_box_is_cell_like(0x0Au64 as *mut c_void));
        // ShortStr (top16 = 0x0001) is an immediate too.
        assert!(!nan_box_is_cell_like(
            0x0001_0000_0000_0061u64 as *mut c_void
        ));
        assert!(!nan_box_is_cell_like(core::ptr::null_mut()));
        // An 8-aligned 48-bit user VA is the real-cell shape.
        assert!(nan_box_is_cell_like(
            0x0000_0001_2345_6788u64 as *mut c_void
        ));
    }

    #[test]
    fn walkable_children_rejects_nan_box_immediates() {
        // A boxed `1` in an `any` field must never be dereferenced as
        // a heap header — the pre-fix wild read behind the 1024-
        // candidate auto-collect SIGSEGV.
        assert!(!unsafe { has_walkable_children(0xFFFE_0000_0000_0001u64 as *mut c_void) });
        assert!(!unsafe { has_walkable_children(core::ptr::null_mut()) });
    }

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
        // 刀 4: ClassLayout = u32 + u32 flags + 3 ptr = 32B, align 8.
        // (Was 24B pre-刀4 / 16B pre-A2 — see
        // torajs-link/src/user_class_layouts_layout.rs OUTER_ENTRY_SIZE
        // for the on-disk emit side that must stay in lockstep with
        // this struct's size.)
        assert_eq!(core::mem::size_of::<ClassLayout>(), 32);
        assert_eq!(core::mem::align_of::<ClassLayout>(), 8);
        assert_eq!(core::mem::offset_of!(ClassLayout, method_table_ptr), 24);
    }

    #[test]
    fn color_constants_match_rc() {
        // Mirrors torajs-rc color.rs — COLOR_SHIFT = 13 (RFC
        // 20260706 chunk 573), mask = 3 << 13.
        assert_eq!(COLOR_SHIFT, 13);
        assert_eq!(COLOR_MASK, 0b11 << 13);
        assert_eq!(COLOR_BLACK, 0x00);
        assert_eq!(COLOR_GRAY, 1 << 13);
        assert_eq!(COLOR_PURPLE, 1 << 14);
        assert_eq!(COLOR_WHITE, 0b11 << 13);
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

    // L3b #16 + RFC 20260706 Phase C — the walk descends into
    // ARR_KIND_HEAP arrays and Arr<Any> (per-slot NaN-box gate in
    // `arr_child_at`); scalar-kind and UNSET slots are not cell
    // pointers and stay element leaves. RFC 20260717 blade 3 — a
    // non-NULL +24 expando makes any non-literal Arr visitable
    // (expando-only walk for scalar kinds), so the fake cell must
    // carry a real props slot.
    #[test]
    fn visitable_arr_gates_on_elem_kind() {
        #[repr(C, align(8))]
        struct FakeArr {
            hdr: HeapHeader,
            len: u64,
            data: u64,
            props: u64, // +24 — ARR_PROPS_OFF mirror
        }
        fn fake_arr(flags: u16, props: u64) -> FakeArr {
            FakeArr {
                hdr: HeapHeader {
                    refcount: 1,
                    type_tag: TAG_ARR,
                    flags,
                },
                len: 0,
                data: 0,
                props,
            }
        }
        let heap = fake_arr(ARR_KIND_HEAP << ARR_ELEM_KIND_SHIFT, 0);
        assert!(unsafe { is_visitable_arr(&heap as *const _ as *mut c_void) });
        let unset = fake_arr(0, 0);
        assert!(!unsafe { is_visitable_arr(&unset as *const _ as *mut c_void) });
        let i64_kind = fake_arr(1 << ARR_ELEM_KIND_SHIFT, 0);
        assert!(!unsafe { is_visitable_arr(&i64_kind as *const _ as *mut c_void) });
        let any_arr = fake_arr(FLAG_ARR_ANY, 0);
        assert!(unsafe { is_visitable_arr(&any_arr as *const _ as *mut c_void) });
        let static_heap = fake_arr(
            FLAG_STATIC_LITERAL | (ARR_KIND_HEAP << ARR_ELEM_KIND_SHIFT),
            0,
        );
        assert!(!unsafe { is_visitable_arr(&static_heap as *const _ as *mut c_void) });
        // blade 3 — a scalar-kind arr with a live expando IS
        // visitable (expando-only), a literal one still is not.
        let scalar_with_props = fake_arr(1 << ARR_ELEM_KIND_SHIFT, 0x1000);
        assert!(unsafe { is_visitable_arr(&scalar_with_props as *const _ as *mut c_void) });
        let literal_with_props = fake_arr(FLAG_STATIC_LITERAL, 0x1000);
        assert!(!unsafe { is_visitable_arr(&literal_with_props as *const _ as *mut c_void) });
    }
}
