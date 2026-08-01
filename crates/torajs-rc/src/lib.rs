//! Universal heap-object header + non-atomic refcount primitives for
//! the torajs AOT TypeScript runtime.
//!
//! Layer 1 of the torajs runtime crate stack: every refcounted heap
//! value carries a [`HeapHeader`] at offset 0; assignment paths call
//! `inc_ref()`, drop paths call `dec_ref()` to decide whether to
//! free + walk children. The C-side substrate (previously inline
//! `__torajs_rc_inc` / `__torajs_rc_dec` in `runtime_str.c`) is
//! replaced by this crate; the FFI surface (the
//! `__torajs_rc_inc` / `__torajs_rc_dec` symbols toolchain-emitted
//! code calls) is preserved exactly via the
//! [`__torajs_rc_inc`] / [`__torajs_rc_dec`] thin shims at the
//! bottom of this file.
//!
//! ## Design — Rust-native, not a C transcription
//!
//! Rather than mirror the C signatures verbatim, the *inner* API is
//! built around Rust idioms:
//!
//! - **`HeapHeader::inc_ref()` / `dec_ref()`** are methods on the
//!   header struct, taking `&mut self`. The static-literal /
//!   refcount-saturate / WeakRef-hook logic lives on the struct,
//!   not in free fns that re-read raw bytes.
//! - **[`DropPolicy`]** is a real enum (`Keep` / `Free`) instead of
//!   a `-> i32` with `0 / 1` magic values. Callers `match` on it.
//! - **[`Color`]** is a 4-variant enum (`Black` / `Gray` / `Purple`
//!   / `White`) for the Bacon-Rajan cycle collector, swapped via
//!   `HeapHeader::set_color()` + read via `header.color()` instead
//!   of `(header.flags & COLOR_MASK) >> COLOR_SHIFT` bit-twiddle at
//!   every call site.
//! - **[`Tag`]** is a 18-variant enum for the per-value type tag
//!   that drives dispatch in `__torajs_value_drop_heap`. Per-type
//!   sub-crates will (in P3+) consume `header.tag()` rather than
//!   re-declaring `#define __TORAJS_TAG_*` constants.
//! - **[`AnySlotTag`]** the same shape for the 16-byte `Array<Any>`
//!   slot tag field (orthogonal to `HeapHeader.flags`).
//!
//! The FFI surface ([`__torajs_rc_inc`] / [`__torajs_rc_dec`]) is a
//! thin pointer-to-reference adapter — null-check + unsafe reborrow
//! + delegate to the method. Less than 10 lines each.
//!
//! ## ABI invariants (must not change)
//!
//! - `HeapHeader` is `#[repr(C, align(8))]` with exactly 8 bytes:
//!   `refcount: u32 @0, type_tag: u16 @4, flags: u16 @6`. Byte-for-
//!   byte mirror of the original C `__torajs_heap_header_t`. Per-
//!   type structs in `runtime_*.c` declare their own copy of this
//!   shape; they are binary-compatible.
//! - The cycle collector reads / writes `flags` directly at the
//!   bit positions encoded by [`COLOR_SHIFT`] / [`COLOR_MASK`].
//!   Layout drift here would silently corrupt the trial-deletion
//!   pass.
//! - The 2-bit color field lives at bits 13-14 (RFC 20260706
//!   chunk 573), disjoint from every flag user. It historically
//!   overlapped [`FLAG_ARR_ANY`] (bit 3) and [`FLAG_FROZEN`]
//!   (bit 4) behind a "use sites are disjoint" assumption that
//!   broke for FROZEN: the collector colors declared-class
//!   instances, freeze marks those too, and scan-black cleared
//!   the freeze bit (silent write-through after `Bun.gc`).
//!
//! ## Non-atomic, single-threaded
//!
//! tora's runtime is single-threaded today (JS spec's single
//! event-loop model). `refcount` is plain `u32` — `AtomicU32` would
//! compile to identical asm under `Ordering::Relaxed` and risks
//! inhibiting LLVM auto-vectorize on batched walks. When threading
//! lands, a new variant API will be added explicitly.
//!
//! ## Safety
//!
//! All methods take `&mut self`, which Rust enforces by reference
//! aliasing rules. The FFI wrappers ([`__torajs_rc_inc`] /
//! [`__torajs_rc_dec`]) take raw `*mut c_void`; callers there
//! guarantee the pointer is null or refers to a live `HeapHeader`.
//! Single-threaded invariant is contract, not enforced.

// Plain `std` crate. Tried `#![no_std]` + custom `panic_handler`
// for tighter staticlib size but it tripped two stable-rustc
// issues:
//
//  1. `cargo test --workspace --release` insists on building the
//     `staticlib` variant under unwind-panics regardless of
//     workspace `[profile.test]` / `[profile.release]` panic
//     settings, so no_std staticlib + precompiled core =
//     "unwinding panics not supported without std" error.
//  2. Multiple Layer-1 no_std staticlibs each defining their own
//     `#[panic_handler]` conflict on the lang item.
//
// Accepting std-flavored staticlibs is the practical answer. The
// `rust_begin_unwind` duplicate between multiple std-bearing
// staticlibs is suppressed at user-binary link time by
// `cc -flto` (LLVM's archive linker tolerates the symbol
// overlap), so every `tr build` user binary links cleanly. Note
// that `cargo test --workspace --release` still hits a duplicate
// at the in-process Rust test binary link — per-crate testing
// (`cargo test -p <crate> --release`) is the acceptance gate
// variant for now (project status memory captures this).
//
// std is a Rust language primitive (not a crates.io dep) so this
// does not violate vision #4 (0 deps): `cargo tree -p torajs-rc`
// shows zero dependencies. Post-LTO dead-strip removes every
// symbol the final user binary doesn't reference — rc_inc /
// rc_dec pull in no std code.

use std::ffi::c_void;

pub mod builtin_proto;
pub mod extensible;
pub mod freeze;
pub mod in_op_any;
pub mod instanceof_any;

// __torajs_value_drop_heap (the universal heap-typed drop dispatch)
// lives in its own torajs-value-drop sub-crate, NOT in torajs-rc.
// Rationale: torajs-rc is in many Cargo dep trees (torajs-arr,
// torajs-anyvalue, etc.) whose own cargo tests stub the symbol
// locally; an rlib-resident dispatch would LTO-collide with those
// stubs ("Linking globals named '__torajs_value_drop_heap': symbol
// multiply defined!"). Keeping the dispatch in a sibling crate
// nobody adds as a Cargo dep ensures the rlib graph stays clean
// while libtorajs_value_drop.a still co-links at `tr build` time.

// ============================================================
// Universal heap-object header
// ============================================================

/// 8-byte aligned header at offset 0 of every refcounted torajs
/// heap object. Public fields because the ABI is fixed
/// (`#[repr(C)]`) and per-type sub-crates / the cycle collector
/// build aggregates around this struct.
///
/// Prefer the inherent methods over direct field manipulation:
/// they encode the static-literal bypass + WeakRef-hook
/// ordering + cycle-color bit positions correctly. The fields
/// are pub only to keep `#[repr(C)]` legal.
#[repr(C, align(8))]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

// Header-flags bit-position constants (incl. the u16 occupancy map)
// live in `flags.rs`; re-exported at crate root just below, same
// shape as `color`.

// Array element-kind field constants (Tag::Arr, flags bits 10-12 —
// Any-dynamic-access RFC 20260704) live in `arr_kind.rs`;
// re-exported at crate root just below, same shape as `color`.

// Cycle-collector color enum + COLOR_SHIFT/COLOR_MASK constants
// live in `color.rs`; re-exported at crate root just below so
// downstream crates can keep writing `torajs_rc::Color` etc.

pub mod any_method;
pub mod any_method_intern;
pub mod any_method_iter;
pub mod any_method_meta;
pub mod arr_kind;
pub mod color;
pub mod flags;
pub mod ns_static;
pub mod null_guard;
pub mod undef_cell;
pub use any_method::{
    ANY_METHOD_ADD, ANY_METHOD_ANCHOR, ANY_METHOD_APPLY, ANY_METHOD_ARR_TO_STRING, ANY_METHOD_AT,
    ANY_METHOD_BIG, ANY_METHOD_BIND, ANY_METHOD_BLINK, ANY_METHOD_BOLD, ANY_METHOD_CALL,
    ANY_METHOD_CATCH, ANY_METHOD_CHAR_AT, ANY_METHOD_CHAR_CODE_AT, ANY_METHOD_CLEAR,
    ANY_METHOD_CODE_POINT_AT, ANY_METHOD_CONCAT, ANY_METHOD_CONSTRUCTOR_SLOT,
    ANY_METHOD_COPY_WITHIN, ANY_METHOD_DEFINE_GETTER, ANY_METHOD_DEFINE_SETTER, ANY_METHOD_DELETE,
    ANY_METHOD_DIFFERENCE, ANY_METHOD_ENDS_WITH, ANY_METHOD_ENTRIES, ANY_METHOD_ERROR_TO_STRING,
    ANY_METHOD_EVERY, ANY_METHOD_EXEC, ANY_METHOD_FILL, ANY_METHOD_FILTER, ANY_METHOD_FINALLY,
    ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX, ANY_METHOD_FIND_LAST, ANY_METHOD_FIND_LAST_INDEX,
    ANY_METHOD_FIXED, ANY_METHOD_FLAT, ANY_METHOD_FLAT_MAP, ANY_METHOD_FN_PROTO_LENGTH_SLOT,
    ANY_METHOD_FN_PROTO_NAME_SLOT, ANY_METHOD_FONTCOLOR, ANY_METHOD_FONTSIZE, ANY_METHOD_FOR_EACH,
    ANY_METHOD_GET, ANY_METHOD_GET_DATE, ANY_METHOD_GET_DAY, ANY_METHOD_GET_DESCRIPTION,
    ANY_METHOD_GET_FULL_YEAR, ANY_METHOD_GET_HOURS, ANY_METHOD_GET_MILLISECONDS,
    ANY_METHOD_GET_MINUTES, ANY_METHOD_GET_MONTH, ANY_METHOD_GET_OR_INSERT, ANY_METHOD_GET_SECONDS,
    ANY_METHOD_GET_SIZE, ANY_METHOD_GET_TIME, ANY_METHOD_GET_TIMEZONE_OFFSET,
    ANY_METHOD_GET_UTC_DATE, ANY_METHOD_GET_UTC_DAY, ANY_METHOD_GET_UTC_FULL_YEAR,
    ANY_METHOD_GET_UTC_HOURS, ANY_METHOD_GET_UTC_MILLISECONDS, ANY_METHOD_GET_UTC_MINUTES,
    ANY_METHOD_GET_UTC_MONTH, ANY_METHOD_GET_UTC_SECONDS, ANY_METHOD_GET_YEAR, ANY_METHOD_HAS,
    ANY_METHOD_HAS_OWN_PROPERTY, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_INTERSECTION,
    ANY_METHOD_IS_DISJOINT_FROM, ANY_METHOD_IS_PROTOTYPE_OF, ANY_METHOD_IS_SUBSET_OF,
    ANY_METHOD_IS_SUPERSET_OF, ANY_METHOD_IS_WELL_FORMED, ANY_METHOD_ITALICS, ANY_METHOD_JOIN,
    ANY_METHOD_KEYS, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_LINK, ANY_METHOD_LOCALE_COMPARE,
    ANY_METHOD_LOOKUP_GETTER, ANY_METHOD_LOOKUP_SETTER, ANY_METHOD_MAP, ANY_METHOD_MATCH,
    ANY_METHOD_MATCH_ALL, ANY_METHOD_NEXT, ANY_METHOD_NORMALIZE, ANY_METHOD_OBJECT_TO_STRING,
    ANY_METHOD_PAD_END, ANY_METHOD_PAD_START, ANY_METHOD_POP, ANY_METHOD_PROPERTY_IS_ENUMERABLE,
    ANY_METHOD_PROTO_GET, ANY_METHOD_PROTO_SET, ANY_METHOD_PUSH, ANY_METHOD_REDUCE,
    ANY_METHOD_REDUCE_RIGHT, ANY_METHOD_REPEAT, ANY_METHOD_REPLACE, ANY_METHOD_REPLACE_ALL,
    ANY_METHOD_REVERSE, ANY_METHOD_SEARCH, ANY_METHOD_SET, ANY_METHOD_SET_DATE,
    ANY_METHOD_SET_FULL_YEAR, ANY_METHOD_SET_HOURS, ANY_METHOD_SET_MILLISECONDS,
    ANY_METHOD_SET_MINUTES, ANY_METHOD_SET_MONTH, ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME,
    ANY_METHOD_SET_UTC_DATE, ANY_METHOD_SET_UTC_FULL_YEAR, ANY_METHOD_SET_UTC_HOURS,
    ANY_METHOD_SET_UTC_MILLISECONDS, ANY_METHOD_SET_UTC_MINUTES, ANY_METHOD_SET_UTC_MONTH,
    ANY_METHOD_SET_UTC_SECONDS, ANY_METHOD_SET_YEAR, ANY_METHOD_SHIFT, ANY_METHOD_SLICE,
    ANY_METHOD_SMALL, ANY_METHOD_SOME, ANY_METHOD_SORT, ANY_METHOD_SPLICE, ANY_METHOD_SPLIT,
    ANY_METHOD_STARTS_WITH, ANY_METHOD_STR_ITERATOR, ANY_METHOD_STRIKE, ANY_METHOD_SUB,
    ANY_METHOD_SUBSTR, ANY_METHOD_SUBSTRING, ANY_METHOD_SUP, ANY_METHOD_SYMBOL_TO_STRING,
    ANY_METHOD_SYMBOL_VALUE_OF, ANY_METHOD_SYMMETRIC_DIFFERENCE, ANY_METHOD_TEST, ANY_METHOD_THEN,
    ANY_METHOD_THROW_TYPE_ERROR, ANY_METHOD_TO_DATE_STRING, ANY_METHOD_TO_EXPONENTIAL,
    ANY_METHOD_TO_FIXED, ANY_METHOD_TO_GMT_STRING, ANY_METHOD_TO_ISO_STRING, ANY_METHOD_TO_JSON,
    ANY_METHOD_TO_LOCALE_DATE_STRING, ANY_METHOD_TO_LOCALE_LOWER_CASE, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_LOCALE_TIME_STRING, ANY_METHOD_TO_LOCALE_UPPER_CASE, ANY_METHOD_TO_LOWER_CASE,
    ANY_METHOD_TO_PRECISION, ANY_METHOD_TO_REVERSED, ANY_METHOD_TO_SORTED, ANY_METHOD_TO_SPLICED,
    ANY_METHOD_TO_STRING, ANY_METHOD_TO_TIME_STRING, ANY_METHOD_TO_UPPER_CASE,
    ANY_METHOD_TO_UTC_STRING, ANY_METHOD_TO_WELL_FORMED, ANY_METHOD_TRIM, ANY_METHOD_TRIM_END,
    ANY_METHOD_TRIM_START, ANY_METHOD_UNION, ANY_METHOD_UNKNOWN, ANY_METHOD_UNSHIFT,
    ANY_METHOD_VALUE_OF, ANY_METHOD_VALUES, ANY_METHOD_WITH, ANY_RPROP_DOT_ALL, ANY_RPROP_FLAGS,
    ANY_RPROP_GLOBAL, ANY_RPROP_IGNORE_CASE, ANY_RPROP_LAST_INDEX, ANY_RPROP_MULTILINE,
    ANY_RPROP_SOURCE, ANY_RPROP_STICKY, ANY_RPROP_UNICODE, ANY_WPROP_ARR_LENGTH,
    any_regexp_prop_id,
};
pub use any_method_intern::any_method_id;
pub use any_method_meta::{any_method_meta, any_method_meta_for};
pub use arr_kind::{
    ARR_ELEM_KIND_MASK, ARR_ELEM_KIND_SHIFT, ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP,
    ARR_KIND_I64, ARR_KIND_UNSET,
};
pub use color::{COLOR_MASK, COLOR_SHIFT, Color};
pub use flags::{
    FLAG_ARR_ANY, FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_LENGTH_RO, FLAG_BUFFERED,
    FLAG_CLASS_METHOD_THIS_FREE, FLAG_CLOSURE_RECV_FIRST, FLAG_DYNOBJ_CLASS_CTOR,
    FLAG_DYNOBJ_RAW_JSON, FLAG_ERROR, FLAG_FN_ASYNC, FLAG_FN_GENERATOR, FLAG_FN_LENGTH_DELETED,
    FLAG_FN_NAME_DELETED, FLAG_FN_PROTO, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED,
    FLAG_SPLIT_BLOCK, FLAG_STATIC_LITERAL, FLAG_SUBCLASSED,
};
pub use ns_static::{
    NS_STATIC_UNKNOWN, NsStaticRow, ns_static_id, ns_static_is_deleted, ns_static_mark_deleted,
    ns_static_meta,
};

// Type tags (`Tag` enum) live in `tag.rs`; re-exported at crate
// root just below, same shape as `color` / `arr_kind`.

pub mod tag;
pub use tag::Tag;

// ============================================================
// Any-slot tag (16-byte Array<Any> slot)
// ============================================================

/// Tag for the 16-byte `Array<Any>` slot `{ tag: u64, value: u64 }`.
/// Orthogonal to `HeapHeader::type_tag` — `ANY_HEAP` slots hold a
/// pointer whose actual type is resolved by reading the pointee's
/// `HeapHeader::type_tag` ([`Tag`]).
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnySlotTag {
    /// `null` (per ES spec §6.1.2).
    Null = 0,
    /// `boolean`.
    Bool = 1,
    /// `int64` (inline value).
    I64 = 2,
    /// `float64` (inline value, bitcast).
    F64 = 3,
    /// Pointer to a heap object — actual type via
    /// [`HeapHeader::type_tag`].
    Heap = 4,
    /// `undefined` (per ES spec §6.1.1; distinct from `null`).
    Undef = 5,
}

// ============================================================
// Decrement verdict
// ============================================================

/// Verdict returned by [`HeapHeader::dec_ref`]. `Free` tells the
/// caller to walk owned children + free the memory; `Keep` says
/// the object still has live refs (or is a static literal — same
/// "don't free" outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    /// Refcount > 0 after decrement, or no decrement happened
    /// (static literal / null caller branch). Caller leaves the
    /// memory alone.
    Keep,
    /// Refcount transitioned to zero. Caller drops children +
    /// frees. The WeakRef hook has already fired by the time
    /// this is returned, so subsequent free is safe.
    Free,
}

// ============================================================
// WeakRef hook (defined in runtime_weakref.c)
// ============================================================

// `__torajs_weakref_target_dying(p)` is called on rc-hit-zero
// before [`DropPolicy::Free`] is returned, so any live `WeakRef`
// pointing at the dying object can NULL its target pointer first.
// Implementation lives in runtime_weakref.c (a global "any
// WeakRef alive" counter gates the body so non-WeakRef programs
// pay only one untaken branch per dec).
unsafe extern "C" {
    fn __torajs_weakref_target_dying(target: *mut c_void);
}

// ============================================================
// HeapHeader methods (the idiomatic core)
// ============================================================

impl HeapHeader {
    /// New header with rc=1, the given tag, and zero flags. Most
    /// callers build the struct directly via `#[repr(C)]` literal
    /// init; this is a convenience for tests / non-init paths.
    #[inline]
    pub const fn new(tag: Tag) -> Self {
        Self {
            refcount: 1,
            type_tag: tag as u16,
            flags: 0,
        }
    }

    /// Read the [`Tag`].
    ///
    /// # Safety
    ///
    /// Assumes `type_tag` holds a valid `Tag` discriminant. Tags
    /// are written by `<Type>_alloc()` functions that all use the
    /// `Tag` enum (post-rewrite) or the matching `#define`
    /// constant (current C glue), so this is upheld by the
    /// runtime invariant.
    #[inline]
    pub fn tag(&self) -> Tag {
        // SAFETY: caller invariant — `type_tag` is one of the 18
        // discriminants. transmute is safe within the enum's
        // repr(u16) numeric domain.
        unsafe { core::mem::transmute::<u16, Tag>(self.type_tag) }
    }

    /// Write a new [`Tag`].
    #[inline]
    pub fn set_tag(&mut self, tag: Tag) {
        self.type_tag = tag as u16;
    }

    /// True iff [`FLAG_STATIC_LITERAL`] is set — rc operations
    /// no-op on this header.
    #[inline]
    pub fn is_static_literal(&self) -> bool {
        self.flags & FLAG_STATIC_LITERAL != 0
    }

    /// True iff [`FLAG_FROZEN`] is set — `Object.freeze`'d.
    #[inline]
    pub fn is_frozen(&self) -> bool {
        self.flags & FLAG_FROZEN != 0
    }

    /// True iff [`FLAG_BUFFERED`] is set — already in the
    /// cycle-collector buffer.
    #[inline]
    pub fn is_buffered(&self) -> bool {
        self.flags & FLAG_BUFFERED != 0
    }

    /// True iff [`FLAG_ARR_ANY`] is set — Array<Any> 16-byte slot
    /// layout. Only meaningful on `Tag::Arr` headers.
    #[inline]
    pub fn is_arr_any(&self) -> bool {
        self.flags & FLAG_ARR_ANY != 0
    }

    /// True iff [`FLAG_SPLIT_BLOCK`] is set — single-malloc block
    /// containing N inline `Substr` structs (str_split output).
    #[inline]
    pub fn is_split_block(&self) -> bool {
        self.flags & FLAG_SPLIT_BLOCK != 0
    }

    /// Read the current cycle-collector [`Color`].
    #[inline]
    pub fn color(&self) -> Color {
        let bits = self.flags & COLOR_MASK;
        // SAFETY: COLOR_MASK is exactly the 2 bits used by `Color`;
        // any value of those 2 bits is one of the 4 variants.
        unsafe { core::mem::transmute::<u16, Color>(bits) }
    }

    /// Write a new cycle-collector [`Color`]. Preserves the other
    /// flags (FROZEN / BUFFERED / etc).
    #[inline]
    pub fn set_color(&mut self, c: Color) {
        self.flags = (self.flags & !COLOR_MASK) | (c as u16);
    }

    /// Mark `BUFFERED` (cycle-collector dedup gate).
    #[inline]
    pub fn set_buffered(&mut self, on: bool) {
        if on {
            self.flags |= FLAG_BUFFERED;
        } else {
            self.flags &= !FLAG_BUFFERED;
        }
    }

    /// Increment the refcount. No-op for static literals.
    /// Returns the new refcount value (useful for tests / debug
    /// asserts; release builds optimize away when ignored).
    #[inline]
    pub fn inc_ref(&mut self) -> u32 {
        if self.is_static_literal() {
            return self.refcount;
        }
        self.refcount += 1;
        self.refcount
    }

    /// Decrement the refcount. Returns [`DropPolicy::Free`] iff
    /// the refcount transitioned to zero (caller must walk + free).
    /// Static literals and the saturation case both return
    /// [`DropPolicy::Keep`].
    ///
    /// On the hit-zero path, fires the runtime_weakref.c hook so
    /// any live `WeakRef` to this object can NULL its target ptr
    /// before the caller's free.
    #[inline]
    pub fn dec_ref(&mut self) -> DropPolicy {
        if self.is_static_literal() {
            return DropPolicy::Keep;
        }
        self.refcount -= 1;
        if self.refcount == 0 {
            // SAFETY: hook is gated internally on a global counter;
            // safe to call with any pointer (it inspects the
            // WeakRef registry by pointer identity).
            unsafe {
                __torajs_weakref_target_dying(self as *mut HeapHeader as *mut c_void);
            }
            DropPolicy::Free
        } else {
            DropPolicy::Keep
        }
    }
}

// FFI shims (`__torajs_rc_inc` / `__torajs_rc_dec`) live in
// `ffi.rs`; they're re-exported at crate root just below so
// external callers can keep writing `torajs_rc::__torajs_rc_inc` /
// `_dec` without rewriting import paths.

pub mod ffi;
pub use ffi::{__torajs_rc_dec, __torajs_rc_inc};

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    // Tests link against the WeakRef hook declared `extern "C"` in
    // the main module. The unit-test binary has no
    // runtime_weakref.c to provide it, so we stub the symbol here.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}

    // ---- Layout invariants ----

    #[test]
    fn header_layout_matches_c_definition() {
        // 8-byte total, fields at offsets 0/4/6 — mirrors the C
        // `__torajs_heap_header_t` and the cycle-collector + per-
        // type struct definitions in runtime_*.c. Drift here
        // would shift every per-type struct's payload offset and
        // silently break ssa_lower's IR const-offset arithmetic.
        assert_eq!(size_of::<HeapHeader>(), 8);
        assert_eq!(align_of::<HeapHeader>(), 8);
        assert_eq!(offset_of!(HeapHeader, refcount), 0);
        assert_eq!(offset_of!(HeapHeader, type_tag), 4);
        assert_eq!(offset_of!(HeapHeader, flags), 6);
    }

    #[test]
    fn tag_discriminants_are_stable_wire_format() {
        // ssa_lower emits these as IR literals; renumbering would
        // mistag every heap allocation in shipped binaries. The
        // assertions also guard against the enum being reordered
        // such that `as u16` produces a different mapping.
        assert_eq!(Tag::Str as u16, 0);
        assert_eq!(Tag::Obj as u16, 1);
        assert_eq!(Tag::Arr as u16, 2);
        assert_eq!(Tag::Closure as u16, 3);
        assert_eq!(Tag::RegExp as u16, 4);
        assert_eq!(Tag::Date as u16, 5);
        assert_eq!(Tag::Reserved6 as u16, 6);
        assert_eq!(Tag::Symbol as u16, 7);
        assert_eq!(Tag::Promise as u16, 8);
        assert_eq!(Tag::Response as u16, 9);
        assert_eq!(Tag::BigInt as u16, 10);
        assert_eq!(Tag::WeakRef as u16, 11);
        assert_eq!(Tag::WeakMap as u16, 12);
        assert_eq!(Tag::WeakSet as u16, 13);
        assert_eq!(Tag::DynObj as u16, 14);
        assert_eq!(Tag::Map as u16, 15);
        assert_eq!(Tag::MapIter as u16, 16);
        assert_eq!(Tag::ArrIter as u16, 17);
        assert_eq!(Tag::AccessorPair as u16, 18);
        assert_eq!(Tag::Set as u16, 19);
    }

    #[test]
    fn any_slot_tags_are_stable_wire_format() {
        assert_eq!(AnySlotTag::Null as u64, 0);
        assert_eq!(AnySlotTag::Bool as u64, 1);
        assert_eq!(AnySlotTag::I64 as u64, 2);
        assert_eq!(AnySlotTag::F64 as u64, 3);
        assert_eq!(AnySlotTag::Heap as u64, 4);
        assert_eq!(AnySlotTag::Undef as u64, 5);
    }

    #[test]
    fn flag_bits_are_disjoint_and_match_c_constants() {
        // The C side uses literal `#define`s; this is the parity
        // check against runtime_str.c bit positions.
        assert_eq!(FLAG_SPLIT_BLOCK, 2);
        assert_eq!(FLAG_STATIC_LITERAL, 4);
        assert_eq!(FLAG_ARR_ANY, 8);
        assert_eq!(FLAG_FROZEN, 16);
        assert_eq!(FLAG_BUFFERED, 32);
        assert_eq!(COLOR_SHIFT, 13);
        assert_eq!(COLOR_MASK, 0b0110_0000_0000_0000); // bits 13-14
        assert_eq!(Color::Black as u16, 0);
        assert_eq!(Color::Gray as u16, 1 << 13);
        assert_eq!(Color::Purple as u16, 1 << 14);
        assert_eq!(Color::White as u16, 0b11 << 13);
    }

    #[test]
    fn tag_private_flags_avoid_shared_header_fields() {
        // RFC 20260713-defprop-residual-cluster chunk A: the color
        // field (bits 13-14) paints EVERY tag when the cycle
        // collector buffers a candidate — a tag-private flag placed
        // there reads back as set after a Purple paint (an Arr looked
        // length-locked, a Closure looked name/length-deleted). No
        // flag, tag-private or not, may overlap COLOR_MASK or
        // FLAG_BUFFERED.
        let shared = COLOR_MASK | FLAG_BUFFERED;
        for f in [
            FLAG_SPLIT_BLOCK,
            FLAG_STATIC_LITERAL,
            FLAG_ARR_ANY,
            FLAG_FROZEN,
            FLAG_ERROR,
            FLAG_NON_EXTENSIBLE,
            FLAG_SEALED,
            FLAG_FN_NAME_DELETED,
            FLAG_FN_LENGTH_DELETED,
            FLAG_ARR_EXOTIC_INDEX,
            FLAG_ARR_LENGTH_RO,
            ARR_ELEM_KIND_MASK,
        ] {
            assert_eq!(f & shared, 0, "flag {f:#06x} overlaps color/buffered");
        }
        // Same-tag flags stay disjoint: Closure = tombstones;
        // Arr = elem-kind + exotic + length-RO (+ ARR_ANY).
        assert_eq!(FLAG_FN_NAME_DELETED & FLAG_FN_LENGTH_DELETED, 0);
        let arr_flags = [
            FLAG_ARR_ANY,
            ARR_ELEM_KIND_MASK,
            FLAG_ARR_EXOTIC_INDEX,
            FLAG_ARR_LENGTH_RO,
        ];
        for (i, a) in arr_flags.iter().enumerate() {
            for b in &arr_flags[i + 1..] {
                assert_eq!(a & b, 0, "Arr flags {a:#06x} / {b:#06x} overlap");
            }
        }
    }

    // ---- Methods ----

    #[test]
    fn inc_ref_increments_and_returns_new_value() {
        let mut h = HeapHeader::new(Tag::Str);
        assert_eq!(h.inc_ref(), 2);
        assert_eq!(h.inc_ref(), 3);
        assert_eq!(h.refcount, 3);
    }

    #[test]
    fn inc_ref_skips_static_literals() {
        let mut h = HeapHeader::new(Tag::Str);
        h.flags |= FLAG_STATIC_LITERAL;
        for _ in 0..100 {
            assert_eq!(h.inc_ref(), 1);
        }
        assert_eq!(h.refcount, 1);
    }

    #[test]
    fn dec_ref_keeps_when_count_above_zero() {
        let mut h = HeapHeader {
            refcount: 3,
            type_tag: Tag::Obj as u16,
            flags: 0,
        };
        assert_eq!(h.dec_ref(), DropPolicy::Keep);
        assert_eq!(h.refcount, 2);
        assert_eq!(h.dec_ref(), DropPolicy::Keep);
        assert_eq!(h.refcount, 1);
    }

    #[test]
    fn dec_ref_signals_free_on_transition_to_zero() {
        let mut h = HeapHeader::new(Tag::Obj);
        assert_eq!(h.dec_ref(), DropPolicy::Free);
        assert_eq!(h.refcount, 0);
    }

    #[test]
    fn dec_ref_skips_static_literals() {
        let mut h = HeapHeader {
            refcount: 1,
            type_tag: Tag::Str as u16,
            flags: FLAG_STATIC_LITERAL,
        };
        for _ in 0..100 {
            assert_eq!(h.dec_ref(), DropPolicy::Keep);
        }
        assert_eq!(h.refcount, 1);
    }

    #[test]
    fn balanced_inc_dec_pair_is_stable() {
        let mut h = HeapHeader::new(Tag::Obj);
        for _ in 0..1000 {
            h.inc_ref();
            assert_eq!(h.dec_ref(), DropPolicy::Keep);
        }
        assert_eq!(h.refcount, 1);
    }

    // ---- Color + flag methods ----

    #[test]
    fn color_round_trips_through_set() {
        let mut h = HeapHeader::new(Tag::Obj);
        assert_eq!(h.color(), Color::Black); // default
        h.set_color(Color::Purple);
        assert_eq!(h.color(), Color::Purple);
        h.set_color(Color::White);
        assert_eq!(h.color(), Color::White);
        h.set_color(Color::Gray);
        assert_eq!(h.color(), Color::Gray);
        h.set_color(Color::Black);
        assert_eq!(h.color(), Color::Black);
    }

    #[test]
    fn set_color_preserves_other_flags() {
        let mut h = HeapHeader::new(Tag::Obj);
        h.flags |= FLAG_FROZEN | FLAG_BUFFERED;
        h.set_color(Color::Purple);
        assert!(h.is_frozen());
        assert!(h.is_buffered());
        assert_eq!(h.color(), Color::Purple);
    }

    #[test]
    fn flag_query_methods_read_correct_bits() {
        let mut h = HeapHeader::new(Tag::Str);
        assert!(!h.is_static_literal());
        assert!(!h.is_frozen());
        assert!(!h.is_buffered());
        assert!(!h.is_arr_any());
        assert!(!h.is_split_block());
        h.flags =
            FLAG_STATIC_LITERAL | FLAG_FROZEN | FLAG_ARR_ANY | FLAG_SPLIT_BLOCK | FLAG_BUFFERED;
        assert!(h.is_static_literal());
        assert!(h.is_frozen());
        assert!(h.is_buffered());
        assert!(h.is_arr_any());
        assert!(h.is_split_block());
    }

    #[test]
    fn set_buffered_toggles_only_buffered_bit() {
        let mut h = HeapHeader::new(Tag::Obj);
        h.flags = FLAG_FROZEN | FLAG_STATIC_LITERAL;
        assert!(!h.is_buffered());
        h.set_buffered(true);
        assert!(h.is_buffered());
        assert!(h.is_frozen()); // preserved
        assert!(h.is_static_literal()); // preserved
        h.set_buffered(false);
        assert!(!h.is_buffered());
        assert!(h.is_frozen());
    }

    #[test]
    fn tag_round_trips_through_set() {
        let mut h = HeapHeader::new(Tag::Str);
        assert_eq!(h.tag(), Tag::Str);
        h.set_tag(Tag::Promise);
        assert_eq!(h.tag(), Tag::Promise);
        assert_eq!(h.type_tag, 8);
        h.set_tag(Tag::DynObj);
        assert_eq!(h.tag(), Tag::DynObj);
        assert_eq!(h.type_tag, 14);
    }

    // FFI wrapper tests live in `ffi.rs`'s inline `#[cfg(test)]`
    // module — they exercise `__torajs_rc_inc` / `__torajs_rc_dec`
    // directly there, and the `__torajs_weakref_target_dying`
    // stub above links into the same test binary.
}
