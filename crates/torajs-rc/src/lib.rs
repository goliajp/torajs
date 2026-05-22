//! Universal heap-object header + non-atomic refcount primitives for
//! the torajs AOT TypeScript runtime.
//!
//! Layer 1 of the torajs runtime crate stack: every refcounted heap
//! value carries a [`HeapHeader`] at offset 0; assignment paths call
//! `inc_ref()`, drop paths call `dec_ref()` to decide whether to
//! free + walk children. The C-side substrate (previously inline
//! `__torajs_rc_inc` / `__torajs_rc_dec` in `runtime_str.c`) is
//! replaced by this crate; the FFI surface (the
//! `__torajs_rc_inc` / `__torajs_rc_dec` symbols emitted by
//! `ssa_inkwell` IR) is preserved exactly via the
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
//! - `flags` bit 3 is shared between `Color::Gray` / `Color::Purple`
//!   / `Color::White` (the upper 2 bits of the COLOR_MASK shift
//!   span) AND [`FLAG_ARR_ANY`] (the Array<Any> 16-byte-slot
//!   marker) AND [`FLAG_FROZEN`] (`Object.freeze` marker). The two
//!   users are disjoint per type — cycle-collector colors run on
//!   `Tag::Obj` / `Tag::Arr` containers, FROZEN is only set on
//!   plain objects, ARR_ANY only on Array<Any>. Auditing the
//!   constants without auditing the use sites would miss this.
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

// Plain `std` crate. We tried `#![cfg_attr(not(test), no_std)]` for
// the staticlib build but the dual `crate-type = ["rlib",
// "staticlib"]` setup forces a single rustc invocation to satisfy
// both shapes, and the `cfg(test)` toggle then leaves the
// staticlib without a `#[panic_handler]` while still asking for
// unwind panics (`panic = "abort"` in workspace profile.release
// only kicks in when the *whole* compilation tree is rebuilt with
// std=core+abort, not on this mixed cfg).
//
// std is part of the Rust language proper (not a crates.io dep)
// so this does NOT violate vision #4 (0 deps): `cargo tree -p
// torajs-rc` still shows zero dependencies. The staticlib carries
// std code, but post-LTO dead-strip removes every symbol the
// final user binary doesn't reference — `__torajs_rc_inc` /
// `__torajs_rc_dec` are the only entry points and they pull in
// no std code (no `String`, no `Vec`, no panic site that survives
// release optimization). Empirical binary-size delta from this
// change is measured during the P2.2 acceptance gate.

use std::ffi::c_void;
use std::ptr::NonNull;

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

// ============================================================
// Header flags — bit positions (mirror runtime_str.c constants)
// ============================================================

/// `str_split` single-malloc block carrying N inline substrs.
pub const FLAG_SPLIT_BLOCK: u16 = 1 << 1;
/// rc_inc / rc_dec / str_free no-op when set (immortal literal).
pub const FLAG_STATIC_LITERAL: u16 = 1 << 2;
/// Array<Any>: 16-byte slots instead of 8. Disjoint user with
/// cycle-collector color bits (see [`Color`] doc).
pub const FLAG_ARR_ANY: u16 = 1 << 3;
/// `Object.freeze(obj)` set — field stores become silent no-ops.
pub const FLAG_FROZEN: u16 = 1 << 4;
/// "this object is in the cycle-collector buffer right now" gate
/// to avoid traversing the buffer for dedup on every `rc_dec`.
pub const FLAG_BUFFERED: u16 = 1 << 5;

/// Bit position of the 2-bit cycle-collector color field.
pub const COLOR_SHIFT: u16 = 3;
/// Mask covering both color bits.
pub const COLOR_MASK: u16 = 0b11 << COLOR_SHIFT;

// ============================================================
// Cycle-collector color
// ============================================================

/// Bacon-Rajan trial-deletion state. Stored in
/// `HeapHeader::flags` at bits 3-4 (see [`COLOR_SHIFT`] /
/// [`COLOR_MASK`]).
///
/// Note the bit overlap with [`FLAG_ARR_ANY`] / [`FLAG_FROZEN`]:
/// cycle-collector traversal only runs on container types
/// (`Tag::Obj` / `Tag::Arr`); ARR_ANY only marks Array<Any>;
/// FROZEN only applies to plain objects. The use sites are
/// disjoint, so the same bits can serve both readers safely —
/// don't repurpose either without an audit of every cycle /
/// freeze / Array<Any> code path.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// In use; no cycle suspicion.
    Black = 0 << COLOR_SHIFT,
    /// Being marked during the current trial-deletion pass.
    Gray = 1 << COLOR_SHIFT,
    /// Buffered as a potential cycle root.
    Purple = 2 << COLOR_SHIFT,
    /// Confirmed garbage; freed by the collect phase.
    White = 3 << COLOR_SHIFT,
}

// ============================================================
// Type tags
// ============================================================

/// Per-heap-object type tag stored in [`HeapHeader::type_tag`].
/// Drives drop dispatch in `__torajs_value_drop_heap` (still in
/// the glue C for now; rewrite of dispatch is queued for the
/// later phase — see `docs/architecture-rewrite.md`).
///
/// Values are stable wire-format; do not renumber. Adding a new
/// type takes the next free integer + a new variant here + a
/// new `case` in the dispatcher.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// `Str` — `[header:8][len:8][bytes:N]`.
    Str = 0,
    /// `Obj` — static-layout class instance / property bag.
    Obj = 1,
    /// `Arr<T>` — head-aware deque.
    Arr = 2,
    /// `Closure` — `{ fn_ptr, env_ptr }` env-first ABI.
    Closure = 3,
    /// `RegExp` — compiled NFA + flags.
    RegExp = 4,
    /// `Date` — `{ ms_since_epoch }`.
    Date = 5,
    /// Boxed `Type::Any` value.
    AnyBox = 6,
    /// `Symbol` — `{ desc_str_ptr }`.
    Symbol = 7,
    /// `Promise<T>` — own drop path (not via value_drop_heap).
    Promise = 8,
    /// `fetch()` `Response`.
    Response = 9,
    /// `BigInt` — sign-magnitude limbs.
    BigInt = 10,
    /// `WeakRef<T>` — `{ target_ptr | null }`.
    WeakRef = 11,
    /// `WeakMap<K, V>`.
    WeakMap = 12,
    /// `WeakSet<K>`.
    WeakSet = 13,
    /// Dynamic-property object (HashMap-backed).
    DynObj = 14,
    /// Strong-ref `Map<K, V>`.
    Map = 15,
    /// `MapIter` — stateful Map iterator.
    MapIter = 16,
    /// `ArrIter` — stateful Array<Any> iterator.
    ArrIter = 17,
}

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

// ============================================================
// FFI wrappers — thin shims for ssa_lower-emitted IR calls
// ============================================================
//
// These keep the exact C ABI (`extern "C"`, `*mut c_void` param,
// `i32` return on dec for legacy 0/1 verdicts) so ssa_inkwell
// doesn't need any IR-side changes. Each wrapper is the
// null-check + reborrow + delegate; all real logic is in the
// methods above.

/// FFI bridge to [`HeapHeader::inc_ref`]. Null-safe.
///
/// # Safety
///
/// `p` is null OR a valid `*mut HeapHeader` pointing to a live
/// header. Single-threaded contract — no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(p: *mut c_void) {
    if let Some(mut header) = NonNull::new(p as *mut HeapHeader) {
        // SAFETY: `p` was non-null per the NonNull match arm;
        // caller invariant says it points to a live header.
        unsafe { header.as_mut() }.inc_ref();
    }
}

/// FFI bridge to [`HeapHeader::dec_ref`]. Null-safe. Returns
/// `1` if the caller must free the object (matches the legacy C
/// `int __torajs_rc_dec` contract that other `runtime_*.c` files
/// already consume), `0` for keep.
///
/// # Safety
///
/// Same as [`__torajs_rc_inc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(p: *mut c_void) -> i32 {
    let Some(mut header) = NonNull::new(p as *mut HeapHeader) else {
        return 0;
    };
    // SAFETY: as above.
    match unsafe { header.as_mut() }.dec_ref() {
        DropPolicy::Free => 1,
        DropPolicy::Keep => 0,
    }
}

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
        assert_eq!(Tag::AnyBox as u16, 6);
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
        assert_eq!(COLOR_SHIFT, 3);
        assert_eq!(COLOR_MASK, 0b11000); // bits 3-4
        assert_eq!(Color::Black as u16, 0);
        assert_eq!(Color::Gray as u16, 0b01000); // bit 3
        assert_eq!(Color::Purple as u16, 0b10000); // bit 4
        assert_eq!(Color::White as u16, 0b11000); // bits 3+4
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

    // ---- FFI wrappers ----

    #[test]
    fn ffi_rc_inc_null_is_noop() {
        unsafe { __torajs_rc_inc(core::ptr::null_mut()) };
    }

    #[test]
    fn ffi_rc_dec_null_returns_zero() {
        let r = unsafe { __torajs_rc_dec(core::ptr::null_mut()) };
        assert_eq!(r, 0);
    }

    #[test]
    fn ffi_rc_inc_increments() {
        let mut h = HeapHeader::new(Tag::Str);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        unsafe { __torajs_rc_inc(p) };
        unsafe { __torajs_rc_inc(p) };
        assert_eq!(h.refcount, 3);
    }

    #[test]
    fn ffi_rc_dec_returns_one_on_hit_zero() {
        let mut h = HeapHeader::new(Tag::Obj);
        let p = &mut h as *mut HeapHeader as *mut c_void;
        assert_eq!(unsafe { __torajs_rc_dec(p) }, 1);
        assert_eq!(h.refcount, 0);
    }

    #[test]
    fn ffi_rc_dec_returns_zero_above_zero() {
        let mut h = HeapHeader {
            refcount: 5,
            type_tag: Tag::Obj as u16,
            flags: 0,
        };
        let p = &mut h as *mut HeapHeader as *mut c_void;
        assert_eq!(unsafe { __torajs_rc_dec(p) }, 0);
        assert_eq!(h.refcount, 4);
    }
}
