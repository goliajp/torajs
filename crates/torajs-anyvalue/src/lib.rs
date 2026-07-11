//! Boxed `Type::Any` value primitives for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-1 substrate built on [`torajs-rc`]. Originally replaced the
//! C-side `Any` ABI defined in the pre-rewrite `runtime_str.c`
//! (`P2.3-a` of the architecture rewrite). Step 7 NaN-box AnyValue
//! cutover (see `docs/v0.7-Phase3-nanbox.md`) replaced the heap
//! `AnyBox` ABI with a u64 immediate carrying a NaN-box payload —
//! the FFI surface ssa_lower binds against is now the
//! `__torajs_anyv_*` family in [`nanbox_ffi`] / [`nanbox_encode`].
//!
//! ## What `AnyBox` is
//!
//! A 24-byte heap struct that holds *any* TypeScript value of type
//! `Type::Any`: every callsite of an Any-typed slot, every
//! `Array<Any>` element, every dynamic-property bag value goes
//! through one. The struct stores:
//!
//! ```text
//! offset 0..7  : header   = HeapHeader { rc:u32, tag=ANY_BOX, flags }
//! offset 8..15 : tag      = i64 one of AnySlotTag::{Null,Bool,I64,F64,Heap,Undef}
//! offset 16..23: value    = i64; inline value or `*mut HeapHeader` cast
//! ```
//!
//! 24 bytes 8-aligned — fits in two cache-line writes for the alloc
//! path. The `value: i64` is interpreted per `tag`:
//!
//! | tag             | value meaning                                  |
//! |-----------------|------------------------------------------------|
//! | `Null` / `Undef`| ignored (canonically 0)                        |
//! | `Bool`          | low bit = 1 truthy / 0 falsy                   |
//! | `I64`           | the integer itself                             |
//! | `F64`           | `f64::from_bits(value as u64)`                 |
//! | `Heap`          | `*mut HeapHeader` (cast through `uintptr_t`)   |
//!
//! ## Design — idiomatic Rust (no C 壳, per the project rule)
//!
//! - **[`AnyBox`]** is a `#[repr(C, align(8))]` struct (because the
//!   ABI is fixed: `Object.freeze` boxes, dynobj buckets, Array<Any>
//!   slots all read fields by const offset). Public fields are
//!   pub because `#[repr(C)]` requires it, but method access (e.g.
//!   `b.tag()`, `b.value()`, `b.heap_payload()`) is what callers
//!   should prefer.
//! - **[`AnyView`]** is a Rust-side enum that *materializes* what
//!   the box holds. The materialization is one-way (read-only —
//!   the box stays the source of truth); it gives downstream Rust
//!   sub-crates a `match`-able value for pretty-printing,
//!   strict-eq, etc. (`AnyView` was renamed from `AnyValue` in
//!   Step 7b — the `AnyValue` symbol is now the NaN-box u64
//!   immediate alias defined in [`nanbox`].)
//! - **[`AnyBox::alloc`]** is the Rust-native constructor. Returns
//!   `NonNull<AnyBox>`. Heap-tagged children get `rc_inc`'d at
//!   alloc time (the box gains ownership).
//! - **[`AnyBox::drop_owned`]** is the Rust-native destructor. Walks
//!   the heap payload if `tag == Heap` (delegating to the per-type
//!   drop dispatch in the C-side `value_drop_heap`, which P3 will
//!   replace with a Rust registry), then `dealloc`s the 24-byte
//!   block. Static-literal flag bypass preserved.
//! - **FFI shims** ssa_lower binds against live in [`nanbox_encode`]
//!   / [`nanbox_ffi`] as the `__torajs_anyv_*` family — they accept
//!   and return NaN-box `AnyValue` immediates. AnyBox is now a
//!   transitional alloc target for the arithmetic dispatch
//!   (`any_arith` / `any_add` still alloc on the heap before the
//!   FFI shim NaN-box-immediate-converts the result for ssa_lower).
//!
//! ## Why `Heap`-tagged children need `value_drop_heap`
//!
//! When the box wraps a `*mut HeapHeader` (`tag == Heap`), drop has
//! to walk that child via the per-type drop dispatch (a Str drop
//! frees its bytes pool slot, an Arr drop walks slots, etc.). The
//! dispatch table currently lives in C (`__torajs_value_drop_heap`
//! in runtime_str.c) and is the work item of P3-onwards. Until
//! then, `drop_owned` calls into that C symbol via an `extern "C"`
//! decl — that is a temporary cross-language call, NOT a "C 壳" in
//! the design sense (the design here is fully Rust; the call into
//! C is a Layer-3 dependency that the rewrite hasn't reached yet).

// Plain `std` crate, matching `torajs-rc`. See that crate's
// header for the full rationale — short version: `cargo test`
// + dual `crate-type = ["rlib", "staticlib"]` + `no_std`
// combine to a precompiled-core panic-strategy mismatch that
// has no clean fix on stable. std staticlibs link cleanly at
// `tr build` time (cc + LLVM-LTO dedup tolerates std symbol
// overlap between Rust-emitted .a's).

use std::ffi::c_void;
use std::ptr::NonNull;

use torajs_rc::{__torajs_rc_inc, AnySlotTag, HeapHeader, Tag};

mod arith;
mod coerce;
mod compare;
mod index_any;
mod iter_any;
mod len_get;
pub(crate) mod member_get;
mod member_props_regexp;
mod member_set;
mod method_bind;
mod method_call;
mod method_call_arr;
mod method_call_closure;
mod method_call_date;
mod method_call_dynobj;
mod method_call_mapset;
mod method_call_num;
mod method_call_regexp;
mod method_call_str;
mod method_call_weak;
mod method_value;
mod name_get;
mod prop_delete;
mod prop_has;

pub mod inspect;

pub mod nanbox;
pub use nanbox::*;

pub(crate) mod nanbox_encode;

// Step 8b-C — ShortStr materialize helpers carved out of
// nanbox_ffi.rs to keep that file's prod LOC ≤ 500 hard limit.
mod nanbox_ffi_materialize;
pub use nanbox_encode::*;

mod nanbox_ffi;
pub use nanbox_ffi::*;

// ============================================================
// AnyView — materialized view of an AnyValue payload
// ============================================================

/// Materialized view of the value an [`AnyBox`] holds. Read-only;
/// `read()` returns a new `AnyView` per call. Useful for `match`
/// at downstream Rust callers (pretty-print, strict-eq, etc.)
/// without re-reading `tag` and `value` by hand.
///
/// `Heap` carries `Option<NonNull<HeapHeader>>` because the box
/// can legitimately store a null pointer when the heap child is
/// `null` (e.g. an explicitly nulled dynobj field). The
/// distinction `tag=Heap, value=NULL` vs `tag=Null` is preserved
/// — they have different semantics in JS (`Object.freeze` on a
/// nulled slot vs a null slot).
///
/// **Renaming history (Step 7b, 2026-05-26):** this type used to
/// be called `AnyValue`. The name `AnyValue` is now reserved for
/// the NaN-box `u64` immediate type defined in [`nanbox`]; the
/// boxed-payload view kept the same shape and just took the
/// `View` suffix. Downstream code that calls `AnyBox::read()`
/// pattern-matches on `AnyView` variants instead of `AnyValue`.
#[derive(Debug, Clone, Copy)]
pub enum AnyView {
    Null,
    Undef,
    Bool(bool),
    I64(i64),
    F64(f64),
    Heap(Option<NonNull<HeapHeader>>),
    /// `tag` value didn't match any known discriminant — should
    /// not happen with a well-formed runtime; defensive variant.
    Unknown,
}

// ============================================================
// Heap-payload rc_inc helper
// ============================================================

/// Refcount-bump the heap payload of an Any-tagged `(tag, value)`
/// pair. Inline-tagged pairs (Null / Undef / Bool / I64 / F64) are
/// no-ops; `Heap` calls `rc_inc(value as *mut c_void)`.
///
/// Used at every site where an Any-tagged payload's ownership is
/// being shared (e.g. dynobj field copy, Array<Any> slot dup)
/// without going through a fresh `AnyBox::alloc`.
#[inline]
pub fn payload_rc_inc(tag: i64, value: i64) {
    if tag == AnySlotTag::Heap as i64 {
        // SAFETY: caller invariant — `value` is either null or a
        // valid `*mut HeapHeader`. `rc_inc` is null-safe.
        unsafe { __torajs_rc_inc(value as *mut c_void) };
    }
}

// ============================================================
// External C-side helpers
//   - `__torajs_value_drop_heap` — per-type drop dispatcher
//     (universal, dispatch table in `torajs-value-drop`). Called
//     by [`nanbox_ffi::__torajs_anyv_rc_dec`] when a cell-tagged
//     AnyValue hits rc 0.
//   - `__torajs_str_eq` — Str byte-equality fast path. Used by
//     [`AnyView::strict_eq`] / `__torajs_anyv_strict_eq` when
//     both heap pointers are Tag::Str. Stays in C until the
//     `torajs-str` rewrite (Layer-2 sub-phase).
// ============================================================

unsafe extern "C" {
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    // P2.3-c — Str-formatting helpers used by `AnyView::to_str` /
    // `__torajs_anyv_to_str`. Each returns a freshly-owned Str
    // (refcount=1) the caller must drop. The implementations stay
    // in runtime_str.c through the Layer-2 (`torajs-str`) rewrite.
    pub(crate) fn __torajs_null_to_str() -> *mut c_void;
    pub(crate) fn __torajs_undefined_to_str() -> *mut c_void;
    pub(crate) fn __torajs_bool_to_str(b: i32) -> *mut c_void;
    pub(crate) fn __torajs_i64_to_str(n: i64) -> *mut c_void;
    pub(crate) fn __torajs_f64_to_str(n: f64) -> *mut c_void;
    pub(crate) fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    // P2.3-d.1 — Str → IEEE 754 number parser per ES §7.1.4.1.5.
    // Reads the Str byte layout starting at the header. Stays in C
    // until the Layer-2 `torajs-str` rewrite ports `strtod` + the
    // ES whitespace / hex / Infinity grammar.
    pub(crate) fn __torajs_str_to_number(p: *const c_void) -> f64;
    // P2.3-d.4 — Str concatenation per ES §13.15.3 step b.iv. Reads
    // both Str layouts (header + len + bytes), allocates a fresh
    // pooled Str, copies left then right bytes into it; returns a
    // freshly-owned Str ptr (refcount = 1) the caller must drop.
    // Stays in C until the Layer-2 `torajs-str` rewrite.
    pub(crate) fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut c_void;
    // P2.3-d.4 — Str dec-ref + dealloc on rc-0. Mirror of the C
    // rc_dec chain for owned Str pointers; used by any_add to drop
    // the two intermediate ToString results before returning the
    // concat.
    pub(crate) fn __torajs_str_drop(s: *mut c_void);
}

// Str heap-byte data offset within the Str layout
// `[header:8][len:8][bytes:N]` — bytes start at byte 16. Mirror of
// the C `__TORAJS_STR_HDR_SIZE` constant; declared here so the
// `to_str` Heap path's placeholder write hits the right offset.
pub(crate) const STR_HDR_SIZE: usize = 16;

// ============================================================
// Strict equality (JS spec §7.2.13 IsStrictlyEqual)
// ============================================================

impl AnyView {
    /// Strict equality per ES §7.2.13. Differs from `==` only in
    /// the heap path, where `Tag::Str` pairs delegate to
    /// byte-comparison via the C-side `__torajs_str_eq`; other
    /// heap types compare by pointer identity (matches the C
    /// fallback).
    ///
    /// NaN-aware (`F64(NaN) != F64(NaN)`), zero-aware
    /// (`F64(+0.0) == F64(-0.0)`), `Null` and `Undef` are equal
    /// only to their own tag.
    pub fn strict_eq(self, other: AnyView) -> bool {
        match (self, other) {
            (AnyView::Null, AnyView::Null) => true,
            (AnyView::Undef, AnyView::Undef) => true,
            (AnyView::Bool(a), AnyView::Bool(b)) => a == b,
            (AnyView::I64(a), AnyView::I64(b)) => a == b,
            (AnyView::F64(a), AnyView::F64(b)) => a == b,
            (AnyView::Heap(la), AnyView::Heap(lb)) => match (la, lb) {
                (None, None) => true,
                (None, _) | (_, None) => false,
                (Some(lp), Some(rp)) if lp == rp => true,
                (Some(lp), Some(rp)) => {
                    // SAFETY: both ptrs are non-null and point to
                    // initialized HeapHeaders by NonNull invariant.
                    let (lh, rh) = unsafe { (lp.as_ref(), rp.as_ref()) };
                    if matches!(lh.tag(), Tag::Str) && matches!(rh.tag(), Tag::Str) {
                        // SAFETY: both pointees are Tag::Str; the
                        // C-side __torajs_str_eq reads the Str
                        // layout starting at the header.
                        unsafe {
                            __torajs_str_eq(lp.as_ptr() as *const u8, rp.as_ptr() as *const u8) != 0
                        }
                    } else {
                        false
                    }
                }
            },
            _ => false,
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith::{ArithOp, any_add, any_arith, tag_is_i64_shaped};
    use crate::compare::{CompareOp, STR_LEN_OFF, any_compare};
    use std::cmp::Ordering;

    // Test binary needs both extern "C" symbols torajs-anyvalue
    // declares: torajs-rc's __torajs_weakref_target_dying (from
    // rc_dec's hit-zero hook) AND `__torajs_value_drop_heap`
    // (called from `nanbox_ffi::__torajs_anyv_rc_dec` for
    // cell-tagged children).
    //
    // The real `__torajs_value_drop_heap` lives in
    // `torajs-value-drop` (P7.i-drop, 2026-05-24); the shipped
    // binary resolves through `libtorajs_value_drop.a`. cargo
    // test for this crate links rlibs but Rust DCE strips the
    // dispatch fn since no Rust call site references it — the
    // local stub satisfies the linker.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut c_void) {}
    /// P2.3-b — payload_eq's Heap path delegates to str_eq when
    /// both sides are Tag::Str. The shipped binary resolves this
    /// from runtime_str.c; tests provide a pointer-identity stub
    /// (suffices for the strict-eq spec: the same heap byte
    /// sequence at the same address is trivially equal).
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64 {
        if a == b { 1 } else { 0 }
    }
    /// P2.3-d.1 — Str → number parser. Shipped binary resolves this
    /// from runtime_str.c; tests provide a sentinel-returning stub
    /// so the Heap+Str branch in `any_to_number` is observable.
    /// Returns 42.0 unconditionally — every test that exercises the
    /// Heap+Str path checks for exactly this value.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_to_number(_p: *const c_void) -> f64 {
        42.0
    }
    /// Step 7d-A test stubs — the new `__torajs_any_*` shims (rewritten
    /// to delegate to `__torajs_anyv_*`) pull `any_to_str` /
    /// `any_add` / `any_arith` into the test binary's reachable
    /// graph through additional NaN-box bridge entry points. Those
    /// inner impls reference Str-construction helpers that live in
    /// `runtime_str.c` / `libtorajs_str.a` in the shipped binary;
    /// the test binary doesn't link them. Provide null-returning
    /// stubs that satisfy the linker for tests which don't exercise
    /// the Str path. (Tests that do exercise Str — e.g. concat —
    /// are tagged `#[ignore]` since they need the real runtime.)
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_null_to_str() -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_undefined_to_str() -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_bool_to_str(_b: i32) -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_i64_to_str(_n: i64) -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_f64_to_str(_n: f64) -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
        core::ptr::null_mut()
    }
    /// Step 8b-C — stub for `materialize_short_str` calls in shim
    /// tests. Returns null; ShortStr unit tests never feed the
    /// returned pointer into rc_dec / value_drop / str_eq, so a
    /// null return is acceptable (the materialize path is only
    /// exercised by integration / conformance tests).
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_alloc(_src: *const u8, _len: i64) -> *mut u8 {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_concat(_a: *const u8, _b: *const u8) -> *mut c_void {
        core::ptr::null_mut()
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_drop(_s: *mut c_void) {}
    /// RFC 20260707 chunk 3 — the shipped binary resolves the
    /// undefined sentinel cell from libtorajs_str.a; tests get a
    /// stable dummy address (identity compares still behave).
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_str_undef() -> *mut u8 {
        static DUMMY: u8 = 0;
        &DUMMY as *const u8 as *mut u8
    }

    #[test]
    fn payload_rc_inc_no_op_on_inline_tags() {
        // Just verifying no panic; no observable state for inline tags.
        payload_rc_inc(0, 0);
        payload_rc_inc(1, 1);
        payload_rc_inc(2, 42);
        payload_rc_inc(3, f64::to_bits(3.14) as i64);
        payload_rc_inc(5, 0);
    }

    #[test]
    fn payload_rc_inc_bumps_on_heap_tag() {
        let mut child = HeapHeader::new(Tag::Str);
        let initial = child.refcount;
        payload_rc_inc(4 /* Heap */, &mut child as *mut _ as i64);
        assert_eq!(child.refcount, initial + 1);
    }

    // ---- P2.3-b: strict equality ----

    #[test]
    fn anyvalue_strict_eq_null_undef() {
        assert!(AnyView::Null.strict_eq(AnyView::Null));
        assert!(AnyView::Undef.strict_eq(AnyView::Undef));
        // Cross-tag: null vs undefined are NOT strict-eq per
        // ES §7.2.13.
        assert!(!AnyView::Null.strict_eq(AnyView::Undef));
        assert!(!AnyView::Undef.strict_eq(AnyView::Null));
    }

    #[test]
    fn anyvalue_strict_eq_bool_i64() {
        assert!(AnyView::Bool(true).strict_eq(AnyView::Bool(true)));
        assert!(AnyView::Bool(false).strict_eq(AnyView::Bool(false)));
        assert!(!AnyView::Bool(true).strict_eq(AnyView::Bool(false)));
        assert!(AnyView::I64(42).strict_eq(AnyView::I64(42)));
        assert!(!AnyView::I64(42).strict_eq(AnyView::I64(43)));
        // Cross-tag: bool vs int are NOT strict-eq even if values
        // could coerce.
        assert!(!AnyView::Bool(true).strict_eq(AnyView::I64(1)));
    }

    #[test]
    fn anyvalue_strict_eq_f64_ieee_semantics() {
        // NaN !== NaN per IEEE 754.
        assert!(!AnyView::F64(f64::NAN).strict_eq(AnyView::F64(f64::NAN)));
        // +0.0 === -0.0 per IEEE 754.
        assert!(AnyView::F64(0.0).strict_eq(AnyView::F64(-0.0)));
        assert!(AnyView::F64(1.5).strict_eq(AnyView::F64(1.5)));
        assert!(!AnyView::F64(1.5).strict_eq(AnyView::F64(2.5)));
        // Infinity equals itself.
        assert!(AnyView::F64(f64::INFINITY).strict_eq(AnyView::F64(f64::INFINITY)));
    }

    #[test]
    fn anyvalue_strict_eq_heap_pointer_identity() {
        let mut h1 = HeapHeader::new(Tag::Obj);
        let mut h2 = HeapHeader::new(Tag::Obj);
        let p1 = NonNull::new(&mut h1 as *mut HeapHeader);
        let p2 = NonNull::new(&mut h2 as *mut HeapHeader);
        assert!(AnyView::Heap(p1).strict_eq(AnyView::Heap(p1)));
        // Different addresses, both Tag::Obj (non-Str) → false.
        assert!(!AnyView::Heap(p1).strict_eq(AnyView::Heap(p2)));
        // Both none → true (null === null on the heap side).
        assert!(AnyView::Heap(None).strict_eq(AnyView::Heap(None)));
        // One null, one not → false.
        assert!(!AnyView::Heap(None).strict_eq(AnyView::Heap(p1)));
    }

    #[test]
    fn anyvalue_strict_eq_str_via_str_eq() {
        // Two Str-tagged headers at the same address — stub
        // __torajs_str_eq returns 1 on pointer identity, so this
        // is true via the byte-equality fast path.
        let mut s = HeapHeader::new(Tag::Str);
        let p = NonNull::new(&mut s as *mut HeapHeader);
        assert!(AnyView::Heap(p).strict_eq(AnyView::Heap(p)));
    }

    // ---- P2.3-d.1: ToNumber coercion ----

    #[test]
    fn anyvalue_to_number_inline_tags() {
        assert_eq!(AnyView::Null.to_number(), 0.0);
        assert!(AnyView::Undef.to_number().is_nan());
        assert_eq!(AnyView::Bool(true).to_number(), 1.0);
        assert_eq!(AnyView::Bool(false).to_number(), 0.0);
        assert_eq!(AnyView::I64(0).to_number(), 0.0);
        assert_eq!(AnyView::I64(42).to_number(), 42.0);
        assert_eq!(AnyView::I64(-7).to_number(), -7.0);
        assert_eq!(AnyView::F64(3.14).to_number(), 3.14);
        // F64 NaN propagates.
        assert!(AnyView::F64(f64::NAN).to_number().is_nan());
        // Unknown defensively → NaN.
        assert!(AnyView::Unknown.to_number().is_nan());
    }

    #[test]
    fn anyvalue_to_number_heap_null_is_zero() {
        // Heap(None) is the "tag=Heap, value=NULL" case — distinct
        // from AnyView::Null tag-wise. ToNumber here matches the
        // C ABI: 0.0 (defensive, not NaN).
        assert_eq!(AnyView::Heap(None).to_number(), 0.0);
    }

    #[test]
    fn anyvalue_to_number_heap_str_delegates_to_str_to_number() {
        // Heap+Str → __torajs_str_to_number; test stub returns 42.0.
        let mut s = HeapHeader::new(Tag::Str);
        let p = NonNull::new(&mut s as *mut HeapHeader);
        assert_eq!(AnyView::Heap(p).to_number(), 42.0);
    }

    #[test]
    fn anyvalue_to_number_heap_non_str_is_nan() {
        // Heap+Obj (or any non-Str) → NaN, matches the C ABI's
        // "objects coerce to NaN" path (pre-valueOf-method era).
        let mut h = HeapHeader::new(Tag::Obj);
        let p = NonNull::new(&mut h as *mut HeapHeader);
        assert!(AnyView::Heap(p).to_number().is_nan());
    }

    // ---- P2.3-d.2: relational compare ----

    /// Build a fake Str heap block backed by a Vec<u8> the caller
    /// owns. Layout: `[header:8][len:u64][bytes:N]`. Returns the
    /// raw pointer + the backing Vec (kept alive by the caller via
    /// the returned guard).
    fn make_str_blob(bytes: &[u8]) -> (Vec<u8>, *const u8) {
        let mut blob = vec![0u8; STR_HDR_SIZE + bytes.len()];
        // Write a Tag::Str HeapHeader at offset 0.
        let h = HeapHeader::new(Tag::Str);
        let h_bytes = unsafe {
            std::slice::from_raw_parts(
                &h as *const HeapHeader as *const u8,
                core::mem::size_of::<HeapHeader>(),
            )
        };
        blob[..h_bytes.len()].copy_from_slice(h_bytes);
        // Write u64 len at offset 8.
        let len = bytes.len() as u64;
        blob[STR_LEN_OFF..STR_LEN_OFF + 8].copy_from_slice(&len.to_ne_bytes());
        // Write payload at offset STR_HDR_SIZE.
        blob[STR_HDR_SIZE..].copy_from_slice(bytes);
        let p = blob.as_ptr();
        (blob, p)
    }

    #[test]
    fn any_compare_inline_lt_le_gt_ge_on_i64() {
        unsafe {
            // 1 < 2 family
            assert!(any_compare(0, 2, 1, 2, 2)); // 1 < 2
            assert!(any_compare(1, 2, 1, 2, 2)); // 1 <= 2
            assert!(!any_compare(2, 2, 1, 2, 2)); // 1 > 2
            assert!(!any_compare(3, 2, 1, 2, 2)); // 1 >= 2
            // equal
            assert!(!any_compare(0, 2, 5, 2, 5));
            assert!(any_compare(1, 2, 5, 2, 5));
            assert!(!any_compare(2, 2, 5, 2, 5));
            assert!(any_compare(3, 2, 5, 2, 5));
        }
    }

    #[test]
    fn any_compare_f64_ieee_semantics() {
        unsafe {
            let one = 1.0_f64.to_bits() as i64;
            let two = 2.0_f64.to_bits() as i64;
            let nan = f64::NAN.to_bits() as i64;
            // 1.0 < 2.0
            assert!(any_compare(0, 3, one, 3, two));
            // NaN < x: false for ALL ops per spec §7.2.13.
            assert!(!any_compare(0, 3, nan, 3, two));
            assert!(!any_compare(1, 3, nan, 3, two));
            assert!(!any_compare(2, 3, nan, 3, two));
            assert!(!any_compare(3, 3, nan, 3, two));
            // x op NaN: also false.
            assert!(!any_compare(0, 3, two, 3, nan));
        }
    }

    #[test]
    fn any_compare_mixed_inline_tags_via_to_number() {
        unsafe {
            // Bool(true)=1 < I64(2)
            assert!(any_compare(0, 1 /* Bool */, 1, 2 /* I64 */, 2));
            // Null=0 < Bool(true)=1
            assert!(any_compare(0, 0 /* Null */, 0, 1 /* Bool */, 1));
            // Undef=NaN compare → false everywhere.
            assert!(!any_compare(0, 5 /* Undef */, 0, 2 /* I64 */, 0));
            assert!(!any_compare(1, 5, 0, 2, 0));
            // I64(5) > Bool(false)=0
            assert!(any_compare(2, 2, 5, 1, 0));
            // I64(0) >= Null=0
            assert!(any_compare(3, 2, 0, 0, 0));
        }
    }

    #[test]
    fn any_compare_str_str_lexicographic() {
        // Different first byte: "abc" vs "abd"
        let (_a, pa) = make_str_blob(b"abc");
        let (_b, pb) = make_str_blob(b"abd");
        unsafe {
            assert!(any_compare(0, 4, pa as i64, 4, pb as i64)); // < true
            assert!(any_compare(1, 4, pa as i64, 4, pb as i64)); // <= true
            assert!(!any_compare(2, 4, pa as i64, 4, pb as i64)); // > false
            assert!(!any_compare(3, 4, pa as i64, 4, pb as i64)); // >= false
        }
    }

    #[test]
    fn any_compare_str_str_length_tiebreak() {
        // Equal prefix, different length: "ab" < "abc"
        let (_a, pa) = make_str_blob(b"ab");
        let (_b, pb) = make_str_blob(b"abc");
        unsafe {
            assert!(any_compare(0, 4, pa as i64, 4, pb as i64));
            assert!(!any_compare(2, 4, pa as i64, 4, pb as i64));
        }
    }

    #[test]
    fn any_compare_str_str_equal() {
        let (_a, pa) = make_str_blob(b"hello");
        let (_b, pb) = make_str_blob(b"hello");
        unsafe {
            assert!(!any_compare(0, 4, pa as i64, 4, pb as i64)); // <
            assert!(any_compare(1, 4, pa as i64, 4, pb as i64)); // <=
            assert!(!any_compare(2, 4, pa as i64, 4, pb as i64)); // >
            assert!(any_compare(3, 4, pa as i64, 4, pb as i64)); // >=
        }
    }

    #[test]
    fn any_compare_str_vs_number_falls_through_to_number() {
        // "5" vs I64(10) — only ONE side is Str so we ToNumber both.
        // ToNumber("5") = 5.0 via the stubbed __torajs_str_to_number
        // (returns 42.0 sentinel) → 42 > 10 → "5" > 10 in this test
        // env. We just verify the path doesn't take str-str branch.
        let (_a, pa) = make_str_blob(b"5");
        unsafe {
            // a="5" via stub maps to 42.0; rhs I64(10) → 10.0;
            // 42 > 10 → Gt true.
            assert!(any_compare(2, 4, pa as i64, 2, 10));
        }
    }

    #[test]
    fn any_compare_unknown_op_returns_false() {
        unsafe {
            // op=99 is not in {0,1,2,3}; defensive false.
            assert!(!any_compare(99, 2, 1, 2, 2));
        }
    }

    #[test]
    fn compare_op_decode_round_trip() {
        assert_eq!(CompareOp::from_i64(0), Some(CompareOp::Lt));
        assert_eq!(CompareOp::from_i64(1), Some(CompareOp::Le));
        assert_eq!(CompareOp::from_i64(2), Some(CompareOp::Gt));
        assert_eq!(CompareOp::from_i64(3), Some(CompareOp::Ge));
        assert_eq!(CompareOp::from_i64(4), None);
        assert_eq!(CompareOp::from_i64(-1), None);
    }

    #[test]
    fn compare_op_apply_canonical_ordering() {
        assert!(CompareOp::Lt.apply(Ordering::Less));
        assert!(!CompareOp::Lt.apply(Ordering::Equal));
        assert!(!CompareOp::Lt.apply(Ordering::Greater));
        assert!(CompareOp::Le.apply(Ordering::Less));
        assert!(CompareOp::Le.apply(Ordering::Equal));
        assert!(!CompareOp::Le.apply(Ordering::Greater));
        assert!(!CompareOp::Gt.apply(Ordering::Less));
        assert!(!CompareOp::Gt.apply(Ordering::Equal));
        assert!(CompareOp::Gt.apply(Ordering::Greater));
        assert!(!CompareOp::Ge.apply(Ordering::Less));
        assert!(CompareOp::Ge.apply(Ordering::Equal));
        assert!(CompareOp::Ge.apply(Ordering::Greater));
    }

    // ---- P2.3-d.3: arithmetic dispatch ----

    /// Decode an [`AnyValue`] immediate returned by the inner
    /// `any_arith` / `any_add` into an [`AnyView`] for `matches!`
    /// assertions. AnyValue is `Copy` so no drop is needed for
    /// primitives; cell-tagged values would need
    /// [`__torajs_anyv_rc_dec`] but the arithmetic dispatch only
    /// returns numeric primitives in the test cases here.
    fn unbox_drop(v: AnyValue) -> AnyView {
        use crate::nanbox::{
            as_bool, as_double, as_int32, as_pointer, is_bool, is_cell, is_double, is_int32,
            is_null, is_undefined,
        };
        if is_null(v) {
            AnyView::Null
        } else if is_undefined(v) {
            AnyView::Undef
        } else if is_bool(v) {
            AnyView::Bool(as_bool(v))
        } else if is_int32(v) {
            AnyView::I64(as_int32(v) as i64)
        } else if is_double(v) {
            AnyView::F64(as_double(v))
        } else if is_cell(v) {
            AnyView::Heap(NonNull::new(as_pointer(v)))
        } else {
            AnyView::Unknown
        }
    }

    #[test]
    fn arith_op_decode_round_trip() {
        assert_eq!(ArithOp::from_i64(0), Some(ArithOp::Sub));
        assert_eq!(ArithOp::from_i64(1), Some(ArithOp::Mul));
        assert_eq!(ArithOp::from_i64(2), Some(ArithOp::Div));
        assert_eq!(ArithOp::from_i64(3), Some(ArithOp::Mod));
        assert_eq!(ArithOp::from_i64(4), None);
        assert_eq!(ArithOp::from_i64(-1), None);
    }

    #[test]
    fn arith_op_apply_basic_ops() {
        // Plain IEEE-754 arithmetic — sanity checks.
        assert_eq!(ArithOp::Sub.apply(10.0, 3.0), 7.0);
        assert_eq!(ArithOp::Mul.apply(4.0, 5.0), 20.0);
        assert_eq!(ArithOp::Div.apply(10.0, 4.0), 2.5);
        // ES §13.9 % — sign of dividend (matches C fmod).
        assert_eq!(ArithOp::Mod.apply(10.0, 3.0), 1.0);
        assert_eq!(ArithOp::Mod.apply(-10.0, 3.0), -1.0);
    }

    #[test]
    fn arith_op_apply_ieee_edge_cases() {
        // Div by zero → ±Infinity per IEEE 754.
        assert_eq!(ArithOp::Div.apply(1.0, 0.0), f64::INFINITY);
        assert_eq!(ArithOp::Div.apply(-1.0, 0.0), f64::NEG_INFINITY);
        // 0/0 → NaN.
        assert!(ArithOp::Div.apply(0.0, 0.0).is_nan());
        // Mod by 0 → NaN.
        assert!(ArithOp::Mod.apply(5.0, 0.0).is_nan());
        // NaN propagates.
        assert!(ArithOp::Sub.apply(f64::NAN, 1.0).is_nan());
        assert!(ArithOp::Mul.apply(2.0, f64::NAN).is_nan());
    }

    #[test]
    fn arith_op_allows_i64_fast_path() {
        assert!(ArithOp::Sub.allows_i64_fast_path());
        assert!(ArithOp::Mul.allows_i64_fast_path());
        assert!(ArithOp::Mod.allows_i64_fast_path());
        // Div explicitly opts OUT (1/2 === 0.5, not 0).
        assert!(!ArithOp::Div.allows_i64_fast_path());
    }

    #[test]
    fn tag_is_i64_shaped_classification() {
        assert!(tag_is_i64_shaped(AnySlotTag::Null as i64));
        assert!(tag_is_i64_shaped(AnySlotTag::Bool as i64));
        assert!(tag_is_i64_shaped(AnySlotTag::I64 as i64));
        // F64, Undef, Heap → not i64-shaped.
        assert!(!tag_is_i64_shaped(AnySlotTag::F64 as i64));
        assert!(!tag_is_i64_shaped(AnySlotTag::Undef as i64));
        assert!(!tag_is_i64_shaped(AnySlotTag::Heap as i64));
    }

    #[test]
    fn any_arith_int_int_returns_i64_tagged() {
        unsafe {
            // 10 - 3 = 7 → I64 (both inputs i64-shaped, Sub, integer).
            let p = any_arith(0, 2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyView::I64(7)));
            // 4 * 5 = 20 → I64.
            let p = any_arith(1, 2, 4, 2, 5);
            assert!(matches!(unbox_drop(p), AnyView::I64(20)));
            // 10 % 3 = 1 → I64.
            let p = any_arith(3, 2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyView::I64(1)));
        }
    }

    #[test]
    fn any_arith_div_always_returns_f64() {
        unsafe {
            // 10 / 4 = 2.5 → F64 (fractional).
            let p = any_arith(2, 2, 10, 2, 4);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x == 2.5));
            // 10 / 5 = 2 → still F64 (Div opts out of integer fast-path).
            let p = any_arith(2, 2, 10, 2, 5);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x == 2.0));
        }
    }

    #[test]
    fn any_arith_f64_input_returns_f64() {
        unsafe {
            // F64 input forces F64 output even if result is integer.
            let two_bits = 2.0_f64.to_bits() as i64;
            let p = any_arith(
                1, /* Mul */
                3, /* F64 */
                two_bits, 2, /* I64 */
                3,
            );
            // 2.0 * 3 = 6.0 → F64 (left side was F64-tagged).
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x == 6.0));
        }
    }

    #[test]
    fn any_arith_bool_null_treated_as_i64_shaped() {
        unsafe {
            // true + true (Mul) — both Bool-tagged → I64 fast-path.
            let p = any_arith(1, 1 /* Bool */, 1, 1 /* Bool */, 1);
            assert!(matches!(unbox_drop(p), AnyView::I64(1)));
            // null - null = 0 → I64.
            let p = any_arith(0, 0 /* Null */, 0, 0 /* Null */, 0);
            assert!(matches!(unbox_drop(p), AnyView::I64(0)));
        }
    }

    #[test]
    fn any_arith_undef_propagates_nan_as_f64() {
        unsafe {
            // undefined * 2 → NaN → F64 (NaN never round-trips through i64).
            let p = any_arith(1, 5 /* Undef */, 0, 2 /* I64 */, 2);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x.is_nan()));
        }
    }

    #[test]
    fn any_arith_unknown_op_returns_nan_f64() {
        unsafe {
            // op=99 — defensive NaN-box.
            let p = any_arith(99, 2, 1, 2, 2);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x.is_nan()));
        }
    }

    #[test]
    fn any_arith_integer_fractional_result_uses_f64() {
        unsafe {
            // I64(1) % I64 doesn't happen here, but I64-1 + I64-1 should
            // be I64. Verify that integer result via Mod that lands on an
            // exact integer DOES use I64.
            let p = any_arith(3 /* Mod */, 2, 17, 2, 5); // 17 % 5 = 2
            assert!(matches!(unbox_drop(p), AnyView::I64(2)));
        }
    }

    // ---- P2.3-d.4: addition (`+`) dispatch ----

    #[test]
    fn any_add_i64_plus_i64_returns_i64() {
        unsafe {
            // 10 + 3 → I64.
            let p = any_add(2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyView::I64(13)));
            // Negative result.
            let p = any_add(2, 3, 2, -10);
            assert!(matches!(unbox_drop(p), AnyView::I64(-7)));
            // Zero result.
            let p = any_add(2, 5, 2, -5);
            assert!(matches!(unbox_drop(p), AnyView::I64(0)));
        }
    }

    #[test]
    fn any_add_bool_null_treated_as_i64_shaped() {
        unsafe {
            // true + 1 → 2 (I64). Both Bool/I64 are i64-shaped.
            let p = any_add(1 /* Bool */, 1, 2 /* I64 */, 1);
            assert!(matches!(unbox_drop(p), AnyView::I64(2)));
            // null + null → 0 (I64).
            let p = any_add(0 /* Null */, 0, 0 /* Null */, 0);
            assert!(matches!(unbox_drop(p), AnyView::I64(0)));
            // false + true → 1 (I64).
            let p = any_add(1 /* Bool */, 0, 1 /* Bool */, 1);
            assert!(matches!(unbox_drop(p), AnyView::I64(1)));
        }
    }

    #[test]
    fn any_add_f64_input_forces_f64() {
        unsafe {
            // F64 + I64 → F64 even if sum is integer-valued.
            let two_bits = 2.0_f64.to_bits() as i64;
            let p = any_add(3 /* F64 */, two_bits, 2 /* I64 */, 3);
            // 2.0 + 3 = 5.0, but F64-tagged input opts out of I64 fast-path.
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x == 5.0));
        }
    }

    #[test]
    fn any_add_fractional_result_uses_f64() {
        unsafe {
            // F64 1.5 + I64 2 → 3.5 (F64).
            let one_half_bits = 1.5_f64.to_bits() as i64;
            let p = any_add(3, one_half_bits, 2, 2);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x == 3.5));
        }
    }

    #[test]
    fn any_add_undef_propagates_nan() {
        unsafe {
            // undefined + 1 → NaN (Undef toNumber = NaN; any +NaN = NaN).
            let p = any_add(5 /* Undef */, 0, 2 /* I64 */, 1);
            assert!(matches!(unbox_drop(p), AnyView::F64(x) if x.is_nan()));
        }
    }

    // (Str-concat path verified end-to-end via bun-parity fixture +
    // conformance — providing fully-functional stubs for the entire
    // any_to_str + str_concat chain would mean re-implementing the
    // Layer-2 str runtime here; not worth the complexity.)
}
