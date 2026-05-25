//! Boxed `Type::Any` value primitives for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-1 substrate built on [`torajs-rc`]. Replaces the C-side
//! `__torajs_any_box` / `__torajs_any_unbox_tag` /
//! `__torajs_any_unbox_value` / `__torajs_any_payload_rc_inc` /
//! `__torajs_any_box_drop` definitions in
//! `crates/torajs-runtime/src/runtime_str.c` (`P2.3-a` of the
//! architecture rewrite, see `docs/architecture-rewrite.md`).
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
//! - **[`AnyValue`]** is a Rust-side enum that *materializes* what
//!   the box holds. The materialization is one-way (read-only —
//!   the box stays the source of truth); it gives downstream Rust
//!   sub-crates a `match`-able value for pretty-printing,
//!   strict-eq, etc.
//! - **[`AnyBox::alloc`]** is the Rust-native constructor. Returns
//!   `NonNull<AnyBox>`. Heap-tagged children get `rc_inc`'d at
//!   alloc time (the box gains ownership).
//! - **[`AnyBox::drop_owned`]** is the Rust-native destructor. Walks
//!   the heap payload if `tag == Heap` (delegating to the per-type
//!   drop dispatch in the C-side `value_drop_heap`, which P3 will
//!   replace with a Rust registry), then `dealloc`s the 24-byte
//!   block. Static-literal flag bypass preserved.
//! - **FFI shims** at the bottom (`__torajs_any_box`,
//!   `__torajs_any_unbox_tag`, `__torajs_any_unbox_value`,
//!   `__torajs_any_payload_rc_inc`, `__torajs_any_box_drop`) are
//!   thin extern-"C" wrappers that ssa_lower IR can call. Each is
//!   a few lines: null-check / pointer-to-reference / delegate to
//!   the inner method. No real logic lives in them.
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

use std::cmp::Ordering;
use std::ffi::c_void;
use std::ptr::NonNull;

use torajs_rc::{__torajs_rc_dec, __torajs_rc_inc, AnySlotTag, HeapHeader, Tag};

// Direct libc malloc / free instead of `std::alloc::{alloc,
// dealloc}`. Three reasons:
//  1. The C-side runtime (runtime_*.c) uses libc malloc/free;
//     reusing the same allocator means the same pool serves
//     both languages — no cross-allocator UB.
//  2. The pre-rewrite C `__torajs_any_box` already used
//     `malloc(24)`; matching it byte-for-byte keeps the ABI
//     contract simple + preserves any pre-rewrite tooling that
//     scans malloc backtraces.
//  3. `extern "C" { fn malloc / free }` is a system primitive
//     declaration, not a crates.io dep — matches vision #4
//     (0 deps).
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

mod ffi;
pub use ffi::*;

pub mod inspect;

// ============================================================
// AnyBox heap struct
// ============================================================

/// 24-byte AnyBox heap value. ABI-locked layout — `#[repr(C,
/// align(8))]` so the const-offset reads ssa_lower emits at every
/// dynobj / Array<Any> / Any-slot site stay byte-identical to the
/// pre-rewrite C struct.
#[repr(C, align(8))]
pub struct AnyBox {
    /// Universal heap-object header. `type_tag` is always
    /// [`Tag::AnyBox`]; `refcount` is owned by `inc_ref`/`dec_ref`
    /// just like every other heap value.
    pub header: HeapHeader,
    /// Discriminant for the boxed payload. Value space is
    /// [`AnySlotTag`]; stored as `i64` because IR emits boxes via
    /// `(i64 tag, i64 value)` pairs.
    pub tag: i64,
    /// Inline payload. Interpretation depends on `tag`; see crate
    /// docs.
    pub value: i64,
}

/// Size in bytes of the [`AnyBox`] heap block. 24 = 8 (header) +
/// 8 (tag) + 8 (value). 8-aligned via `#[repr(C, align(8))]`.
const ANY_BOX_SIZE: usize = 24;

impl AnyBox {
    /// Allocate a new owned `AnyBox` with refcount 1 and the given
    /// payload. For [`AnySlotTag::Heap`], `rc_inc`s the child
    /// pointer so the box becomes an owner of it.
    ///
    /// Returns `NonNull<AnyBox>` — caller owns the allocation and
    /// must eventually call [`AnyBox::drop_owned`] (or, from C,
    /// the [`__torajs_any_box_drop`] FFI shim) to free it.
    pub fn alloc(tag: AnySlotTag, value: i64) -> NonNull<AnyBox> {
        // SAFETY: libc `malloc(24)` returns either null on OOM or
        // a 24-byte block aligned for at least pointer alignment.
        // 24 % 8 == 0 and libc malloc on every supported platform
        // returns 16-byte-aligned (or better) blocks, so the
        // 8-alignment requirement of `AnyBox` is satisfied.
        let raw = unsafe { malloc(ANY_BOX_SIZE) as *mut AnyBox };
        let ptr =
            NonNull::new(raw).unwrap_or_else(|| torajs_abort::abort_with(b"AnyBox alloc OOM"));
        // SAFETY: just-allocated, exclusive ownership, layout
        // matches AnyBox.
        unsafe {
            ptr.as_ptr().write(AnyBox {
                header: HeapHeader::new(Tag::AnyBox),
                tag: tag as i64,
                value,
            });
            if matches!(tag, AnySlotTag::Heap) {
                __torajs_rc_inc(value as *mut c_void);
            }
        }
        ptr
    }

    /// Read the [`AnySlotTag`].
    ///
    /// Returns `None` if `self.tag` doesn't match any known
    /// discriminant — defensive against IR-side bugs that pass a
    /// bad tag (in practice `ssa_lower` only emits valid tags, but
    /// the runtime invariant should be checkable).
    #[inline]
    pub fn slot_tag(&self) -> Option<AnySlotTag> {
        match self.tag {
            0 => Some(AnySlotTag::Null),
            1 => Some(AnySlotTag::Bool),
            2 => Some(AnySlotTag::I64),
            3 => Some(AnySlotTag::F64),
            4 => Some(AnySlotTag::Heap),
            5 => Some(AnySlotTag::Undef),
            _ => None,
        }
    }

    /// Materialize the box's contents as an [`AnyValue`]. Read-
    /// only; the box itself stays the source of truth.
    #[inline]
    pub fn read(&self) -> AnyValue {
        match self.slot_tag() {
            Some(AnySlotTag::Null) => AnyValue::Null,
            Some(AnySlotTag::Undef) => AnyValue::Undef,
            Some(AnySlotTag::Bool) => AnyValue::Bool(self.value != 0),
            Some(AnySlotTag::I64) => AnyValue::I64(self.value),
            Some(AnySlotTag::F64) => AnyValue::F64(f64::from_bits(self.value as u64)),
            Some(AnySlotTag::Heap) => AnyValue::Heap(NonNull::new(self.value as *mut HeapHeader)),
            None => AnyValue::Unknown,
        }
    }

    /// Drop an owned `AnyBox`. Decrements the box's refcount; if
    /// the count transitions to zero, walks the heap payload (if
    /// any) and `dealloc`s the 24-byte block.
    ///
    /// The static-literal flag bypass is honored — boxes flagged
    /// as immortal literals neither dec nor free.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by [`AnyBox::alloc`] (so the
    /// layout matches `ANY_BOX_LAYOUT`) AND the caller must hold
    /// exclusive ownership of the underlying allocation when the
    /// refcount hits zero. The standard `rc_dec` contract.
    pub unsafe fn drop_owned(ptr: NonNull<AnyBox>) {
        let b = unsafe { ptr.as_ref() };
        if b.header.is_static_literal() {
            return;
        }
        // SAFETY: ptr is owned exclusively per the safety contract.
        let dec = unsafe { __torajs_rc_dec(ptr.as_ptr() as *mut c_void) };
        if dec == 0 {
            // Shared; another owner is still alive.
            return;
        }
        // rc transitioned to zero — walk heap child (if any) and
        // free the box.
        let tag = b.tag;
        let value = b.value;
        if tag == AnySlotTag::Heap as i64 {
            // SAFETY: cross-language call to the C-side per-type
            // drop dispatcher (still in runtime_str.c pre-P3). The
            // child pointer was rc_inc'd at alloc; value_drop_heap
            // does the matching rc_dec + per-type teardown.
            unsafe { __torajs_value_drop_heap(value as *mut c_void) };
        }
        // SAFETY: ptr was libc-malloc'd in `AnyBox::alloc` and rc
        // is now zero, so we exclusively own the bytes; `free`
        // is the matching deallocator.
        unsafe { free(ptr.as_ptr() as *mut c_void) };
    }
}

// ============================================================
// AnyValue — materialized view
// ============================================================

/// Materialized view of the value an [`AnyBox`] holds. Read-only;
/// `read()` returns a new `AnyValue` per call. Useful for `match`
/// at downstream Rust callers (pretty-print, strict-eq, etc.)
/// without re-reading `tag` and `value` by hand.
///
/// `Heap` carries `Option<NonNull<HeapHeader>>` because the box
/// can legitimately store a null pointer when the heap child is
/// `null` (e.g. an explicitly nulled dynobj field). The
/// distinction `tag=Heap, value=NULL` vs `tag=Null` is preserved
/// — they have different semantics in JS (`Object.freeze` on a
/// nulled slot vs a null slot).
#[derive(Debug, Clone, Copy)]
pub enum AnyValue {
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
//   - `__torajs_value_drop_heap` — per-type drop dispatcher, used
//     by `AnyBox::drop_owned` to walk a Heap-tagged child. Still
//     lives in runtime_str.c; the cross-language call collapses
//     to Rust-to-Rust in the later phase that ports the dispatch
//     to Rust.
//   - `__torajs_str_eq` — Str byte-equality fast path. Used by
//     [`AnyValue::strict_eq`] / `__torajs_any_payload_eq` when
//     both heap pointers are Tag::Str. Stays in C until the
//     `torajs-str` rewrite (Layer-2 sub-phase).
// ============================================================

unsafe extern "C" {
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
    // P2.3-c — Str-formatting helpers used by `AnyValue::to_str` /
    // `__torajs_any_to_str`. Each returns a freshly-owned Str
    // (refcount=1) the caller must drop. The implementations stay
    // in runtime_str.c through the Layer-2 (`torajs-str`) rewrite.
    fn __torajs_null_to_str() -> *mut c_void;
    fn __torajs_undefined_to_str() -> *mut c_void;
    fn __torajs_bool_to_str(b: i32) -> *mut c_void;
    fn __torajs_i64_to_str(n: i64) -> *mut c_void;
    fn __torajs_f64_to_str(n: f64) -> *mut c_void;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    // P2.3-d.1 — Str → IEEE 754 number parser per ES §7.1.4.1.5.
    // Reads the Str byte layout starting at the header. Stays in C
    // until the Layer-2 `torajs-str` rewrite ports `strtod` + the
    // ES whitespace / hex / Infinity grammar.
    fn __torajs_str_to_number(p: *const c_void) -> f64;
    // P2.3-d.4 — Str concatenation per ES §13.15.3 step b.iv. Reads
    // both Str layouts (header + len + bytes), allocates a fresh
    // pooled Str, copies left then right bytes into it; returns a
    // freshly-owned Str ptr (refcount = 1) the caller must drop.
    // Stays in C until the Layer-2 `torajs-str` rewrite.
    fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut c_void;
    // P2.3-d.4 — Str dec-ref + dealloc on rc-0. Mirror of the C
    // rc_dec chain for owned Str pointers; used by any_add to drop
    // the two intermediate ToString results before returning the
    // concat.
    fn __torajs_str_drop(s: *mut c_void);
}

// Str heap-byte data offset within the Str layout
// `[header:8][len:8][bytes:N]` — bytes start at byte 16. Mirror of
// the C `__TORAJS_STR_HDR_SIZE` constant; declared here so the
// `to_str` Heap path's placeholder write hits the right offset.
const STR_HDR_SIZE: usize = 16;

// ============================================================
// ToString coercion (JS spec §7.1.17)
// ============================================================

/// Tag-dispatch ToString for a packed `(tag, value)` pair.
/// Always returns a freshly owned `*mut Str` (refcount = 1)
/// that the caller is responsible for dropping.
///
/// - `Null` → `"null"` (via [`__torajs_null_to_str`])
/// - `Undef` → `"undefined"` (per ES §7.1.17.1)
/// - `Bool` → `"true"` / `"false"`
/// - `I64` → decimal print
/// - `F64` → IEEE pretty-print (`f64::to_string`-like via the C
///   helper, kept C through Layer-2 so format semantics stay
///   identical to bun's `console.log`)
/// - `Heap` + `Tag::Str` → rc_inc the same Str pointer + return
///   (no new alloc; caller owns one ref)
/// - `Heap` + other tag → `"[object]"` placeholder until P3 lands
///   per-type pretty-print
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`. The returned pointer is valid until the caller
/// `drop`s it (matches the pre-rewrite C contract).
pub(crate) unsafe fn any_to_str(tag: i64, value: i64) -> *mut c_void {
    if tag == AnySlotTag::Null as i64 {
        return unsafe { __torajs_null_to_str() };
    }
    if tag == AnySlotTag::Undef as i64 {
        return unsafe { __torajs_undefined_to_str() };
    }
    if tag == AnySlotTag::Bool as i64 {
        return unsafe { __torajs_bool_to_str((value != 0) as i32) };
    }
    if tag == AnySlotTag::I64 as i64 {
        return unsafe { __torajs_i64_to_str(value) };
    }
    if tag == AnySlotTag::F64 as i64 {
        return unsafe { __torajs_f64_to_str(f64::from_bits(value as u64)) };
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *mut HeapHeader;
        if child.is_null() {
            return unsafe { __torajs_null_to_str() };
        }
        // SAFETY: child is non-null per the check above; runtime
        // invariant says it points to a valid header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // Tag::Str case: just rc_inc + return; the caller now
            // owns one (additional) reference.
            unsafe { __torajs_rc_inc(child as *mut c_void) };
            return child as *mut c_void;
        }
        // Object placeholder. Replaced by per-type pretty-print
        // when P3 lands proper ToString dispatch.
        const PLACEHOLDER: &[u8] = b"[object]";
        // SAFETY: str_alloc_pooled returns a Str-shaped heap with
        // header + len fields written; the body slot starts at
        // `STR_HDR_SIZE` and is `len` bytes wide. We write
        // exactly 8 bytes there.
        unsafe {
            let p = __torajs_str_alloc_pooled(PLACEHOLDER.len() as u64);
            core::ptr::copy_nonoverlapping(
                PLACEHOLDER.as_ptr(),
                p.add(STR_HDR_SIZE),
                PLACEHOLDER.len(),
            );
            p as *mut c_void
        }
    } else {
        // Unknown tag (defensive): treat as null.
        unsafe { __torajs_null_to_str() }
    }
}

// ============================================================
// Strict equality (JS spec §7.2.13 IsStrictlyEqual)
// ============================================================

impl AnyValue {
    /// Strict equality per ES §7.2.13. Differs from `==` only in
    /// the heap path, where `Tag::Str` pairs delegate to
    /// byte-comparison via the C-side `__torajs_str_eq`; other
    /// heap types compare by pointer identity (matches the C
    /// fallback).
    ///
    /// NaN-aware (`F64(NaN) != F64(NaN)`), zero-aware
    /// (`F64(+0.0) == F64(-0.0)`), `Null` and `Undef` are equal
    /// only to their own tag.
    pub fn strict_eq(self, other: AnyValue) -> bool {
        match (self, other) {
            (AnyValue::Null, AnyValue::Null) => true,
            (AnyValue::Undef, AnyValue::Undef) => true,
            (AnyValue::Bool(a), AnyValue::Bool(b)) => a == b,
            (AnyValue::I64(a), AnyValue::I64(b)) => a == b,
            (AnyValue::F64(a), AnyValue::F64(b)) => a == b,
            (AnyValue::Heap(la), AnyValue::Heap(lb)) => match (la, lb) {
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

/// Same-tag payload equality. Caller asserts tags match by
/// passing the same `tag` field for both sides; this function
/// only compares the value fields within that single tag.
///
/// Used internally by the FFI shims that read the box `tag`
/// field once for the short-circuit and then deferred the
/// value-payload check here.
pub(crate) fn payload_eq(tag: i64, lv: i64, rv: i64) -> bool {
    match tag {
        x if x == AnySlotTag::Null as i64 || x == AnySlotTag::Undef as i64 => true,
        x if x == AnySlotTag::Bool as i64 || x == AnySlotTag::I64 as i64 => lv == rv,
        x if x == AnySlotTag::F64 as i64 => {
            // IEEE 754 ==: NaN != NaN, +0 == -0. Bitcast-then-
            // compare-as-f64 gives that semantics directly.
            f64::from_bits(lv as u64) == f64::from_bits(rv as u64)
        }
        x if x == AnySlotTag::Heap as i64 => {
            let lp = lv as *const HeapHeader;
            let rp = rv as *const HeapHeader;
            if lp == rp {
                return true;
            }
            if lp.is_null() || rp.is_null() {
                return false;
            }
            // SAFETY: both ptrs non-null + caller guarantees they
            // point to live HeapHeaders (single-threaded runtime
            // invariant).
            let (lh, rh) = unsafe { (&*lp, &*rp) };
            if matches!(lh.tag(), Tag::Str) && matches!(rh.tag(), Tag::Str) {
                // SAFETY: both are Tag::Str; __torajs_str_eq
                // matches the C layout.
                unsafe { __torajs_str_eq(lp as *const u8, rp as *const u8) != 0 }
            } else {
                false
            }
        }
        _ => false,
    }
}

// ============================================================
// ToNumber coercion (JS spec §7.1.4)
// ============================================================

/// Tag-dispatch `ToNumber` for a packed `(tag, value)` pair —
/// mirrors ES §7.1.4 over tora's tagged-Any subset:
///
/// - `Null` → `0.0`
/// - `Undef` → `NaN`
/// - `Bool` → `1.0` / `0.0`
/// - `I64` → cast to `f64`
/// - `F64` → bitcast `i64`-bits → `f64`
/// - `Heap` + null pointer → `0.0` (defensive — `Heap`-tag NULL
///   doesn't carry numeric semantics; the C ABI returned 0 here)
/// - `Heap` + [`Tag::Str`] → parse via [`__torajs_str_to_number`]
/// - `Heap` + other tag → `NaN` (objects coerce to NaN until the
///   `valueOf` method dispatch lands in a later phase)
/// - unknown tag → `NaN` (defensive)
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*const HeapHeader` pointing to a live heap
/// object.
pub(crate) unsafe fn any_to_number(tag: i64, value: i64) -> f64 {
    if tag == AnySlotTag::Null as i64 {
        return 0.0;
    }
    if tag == AnySlotTag::Undef as i64 {
        return f64::NAN;
    }
    if tag == AnySlotTag::Bool as i64 {
        return if value != 0 { 1.0 } else { 0.0 };
    }
    if tag == AnySlotTag::I64 as i64 {
        return value as f64;
    }
    if tag == AnySlotTag::F64 as i64 {
        return f64::from_bits(value as u64);
    }
    if tag == AnySlotTag::Heap as i64 {
        let child = value as *const HeapHeader;
        if child.is_null() {
            return 0.0;
        }
        // SAFETY: child non-null per the check above; runtime
        // invariant says it points to a live heap header.
        let h = unsafe { &*child };
        if matches!(h.tag(), Tag::Str) {
            // SAFETY: child is Tag::Str-headed; the C-side
            // __torajs_str_to_number reads the Str layout from
            // the header.
            return unsafe { __torajs_str_to_number(child as *const c_void) };
        }
        return f64::NAN;
    }
    f64::NAN
}

impl AnyValue {
    /// `ToNumber` per ES §7.1.4 — idiomatic-Rust mirror of
    /// [`any_to_number`] for already-materialized `AnyValue`s.
    /// Same per-tag rules; the `Heap` case delegates to the C
    /// `__torajs_str_to_number` for `Tag::Str` and returns `NaN`
    /// for every other heap type.
    pub fn to_number(self) -> f64 {
        match self {
            AnyValue::Null => 0.0,
            AnyValue::Undef => f64::NAN,
            AnyValue::Bool(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            AnyValue::I64(n) => n as f64,
            AnyValue::F64(n) => n,
            AnyValue::Heap(None) => 0.0,
            AnyValue::Heap(Some(p)) => {
                // SAFETY: NonNull invariant — points to a live
                // HeapHeader.
                let h = unsafe { p.as_ref() };
                if matches!(h.tag(), Tag::Str) {
                    // SAFETY: pointer is Tag::Str-headed.
                    unsafe { __torajs_str_to_number(p.as_ptr() as *const c_void) }
                } else {
                    f64::NAN
                }
            }
            AnyValue::Unknown => f64::NAN,
        }
    }
}

// ============================================================
// Relational comparison (JS spec §7.2.13 IsLessThan + §13.10)
// ============================================================

/// Byte offset of the `u64 len` field inside the Str heap layout
/// `[header:8][len:8][bytes:N]`. Used by [`any_compare`] for the
/// String-String lexicographic byte-compare path. Stays in C until
/// the Layer-2 `torajs-str` rewrite.
const STR_LEN_OFF: usize = 8;

/// Op code for ordering compare per ssa_lower's emission.
/// Mirror of the C `__torajs_any_compare` switch on the `op`
/// argument: 0=Lt, 1=Le, 2=Gt, 3=Ge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    /// Decode the i64 wire format ssa_lower emits.
    fn from_i64(op: i64) -> Option<CompareOp> {
        match op {
            0 => Some(CompareOp::Lt),
            1 => Some(CompareOp::Le),
            2 => Some(CompareOp::Gt),
            3 => Some(CompareOp::Ge),
            _ => None,
        }
    }

    /// Apply the op to a canonical `Ordering` result. NaN is
    /// handled by the caller (all four ops return `false` when
    /// either operand is NaN per ES §7.2.13).
    #[inline]
    fn apply(self, cmp: Ordering) -> bool {
        match self {
            CompareOp::Lt => cmp.is_lt(),
            CompareOp::Le => cmp.is_le(),
            CompareOp::Gt => cmp.is_gt(),
            CompareOp::Ge => cmp.is_ge(),
        }
    }
}

/// Returns `true` iff the `(tag, value)` pair points to a live
/// heap object tagged [`Tag::Str`]. Null and non-Heap tags return
/// `false`.
///
/// # Safety
///
/// If `tag == AnySlotTag::Heap as i64`, `value` must be null or a
/// valid `*const HeapHeader`.
#[inline]
unsafe fn is_heap_str(tag: i64, value: i64) -> bool {
    if tag != AnySlotTag::Heap as i64 {
        return false;
    }
    let p = value as *const HeapHeader;
    if p.is_null() {
        return false;
    }
    // SAFETY: non-null + runtime invariant says it points to a
    // live heap header.
    matches!(unsafe { (*p).tag() }, Tag::Str)
}

/// Lexicographic byte compare of two Str-tagged heap pointers,
/// matching ES §7.2.13 step 4.b's "Is Less Than" tie-break by
/// length. Both `la` / `ra` must be non-null `*const u8` pointing
/// at a Str's HeapHeader; the layout is `[header:8][len:u64@8]
/// [bytes:len@16]`.
///
/// # Safety
///
/// `la` and `ra` must be non-null and point to live Str heap
/// objects. Caller guarantees by virtue of `is_heap_str` having
/// returned true for both.
unsafe fn compare_str_lexicographic(la: i64, ra: i64) -> Ordering {
    let la = la as *const u8;
    let ra = ra as *const u8;
    // SAFETY: la/ra non-null per caller invariant; layout-aware
    // unaligned reads at byte offset 8 (u64-aligned since the
    // Str heap is 8-aligned).
    let (l_len, r_len) = unsafe {
        (
            (la.add(STR_LEN_OFF) as *const u64).read() as usize,
            (ra.add(STR_LEN_OFF) as *const u64).read() as usize,
        )
    };
    let min_len = l_len.min(r_len);
    // SAFETY: byte payload starts at offset STR_HDR_SIZE; each is
    // at least min_len bytes long (we took the min).
    let (lb, rb) = unsafe {
        (
            std::slice::from_raw_parts(la.add(STR_HDR_SIZE), min_len),
            std::slice::from_raw_parts(ra.add(STR_HDR_SIZE), min_len),
        )
    };
    match lb.cmp(rb) {
        Ordering::Equal => l_len.cmp(&r_len),
        other => other,
    }
}

/// `<`, `<=`, `>`, `>=` on two Any-tagged `(tag, value)` pairs per
/// ES §7.2.13 IsLessThan + §13.10. Both sides go through
/// `ToPrimitive(hint=Number)`; if BOTH result in String the path
/// is a lexicographic byte-compare, otherwise both run through
/// ToNumber and IEEE 754 compare. NaN makes ALL ops return
/// `false`.
///
/// Returns `false` defensively for any unknown `op` value.
///
/// # Safety
///
/// If either tag is `Heap`, the corresponding value must be null
/// or a valid `*mut HeapHeader`. ToNumber's Heap+Str path
/// delegates to the still-C `__torajs_str_to_number`, which
/// requires the pointer be Tag::Str-headed.
pub(crate) unsafe fn any_compare(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> bool {
    let op = match CompareOp::from_i64(op) {
        Some(o) => o,
        None => return false,
    };
    // SAFETY: caller invariant — propagated.
    let l_is_str = unsafe { is_heap_str(lt, lv) };
    let r_is_str = unsafe { is_heap_str(rt, rv) };
    let cmp = if l_is_str && r_is_str {
        // SAFETY: is_heap_str checked both pointers non-null + Str-
        // headed; compare_str_lexicographic's invariants hold.
        unsafe { compare_str_lexicographic(lv, rv) }
    } else {
        // SAFETY: caller invariant — propagated to any_to_number.
        let l = unsafe { any_to_number(lt, lv) };
        let r = unsafe { any_to_number(rt, rv) };
        if l.is_nan() || r.is_nan() {
            return false;
        }
        // partial_cmp is total for non-NaN f64 (we just excluded
        // NaN above); use unsafe-unchecked to avoid the Result path
        // pulling Rust's panic machinery into the user binary
        // (polish A3).
        unsafe { l.partial_cmp(&r).unwrap_unchecked() }
    };
    op.apply(cmp)
}

// ============================================================
// Arithmetic dispatch (JS spec §13.6 / §13.7 / §13.8 / §13.9)
// ============================================================

/// Op code for `-`, `*`, `/`, `%` per ssa_lower's emission. Mirror
/// of the C `__torajs_any_arith` switch on the `op` argument:
/// 0=Sub, 1=Mul, 2=Div, 3=Mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArithOp {
    Sub,
    Mul,
    Div,
    Mod,
}

impl ArithOp {
    /// Decode the i64 wire format ssa_lower emits.
    fn from_i64(op: i64) -> Option<ArithOp> {
        match op {
            0 => Some(ArithOp::Sub),
            1 => Some(ArithOp::Mul),
            2 => Some(ArithOp::Div),
            3 => Some(ArithOp::Mod),
            _ => None,
        }
    }

    /// Apply the op to two already-ToNumber-d operands. ES §13.9
    /// `%` matches C's `fmod` (sign of dividend; NaN on `y == 0`);
    /// Rust's `f64 % f64` lowers to `fmod` on every host we target,
    /// so the Mod arm is a one-liner with no special-casing.
    #[inline]
    fn apply(self, l: f64, r: f64) -> f64 {
        match self {
            ArithOp::Sub => l - r,
            ArithOp::Mul => l * r,
            ArithOp::Div => l / r,
            ArithOp::Mod => l % r,
        }
    }

    /// Whether the integer fast-path applies to this op. `Div`
    /// always yields f64 even for integer operands (`1/2 === 0.5`,
    /// not `0`), so it's excluded; the rest qualify.
    #[inline]
    fn allows_i64_fast_path(self) -> bool {
        !matches!(self, ArithOp::Div)
    }
}

/// Whether an Any tag's ToNumber result is "i64-shaped" — i.e.
/// always an exact integer in i64 range. Null=0, Bool=0/1,
/// I64=value all qualify; F64 doesn't (may be fractional), Undef
/// doesn't (ToNumber → NaN), Heap+anything doesn't (Str parse can
/// produce f64, object → NaN). Used by `any_arith` to decide the
/// I64-vs-F64 boxing of integer-valued results.
#[inline]
fn tag_is_i64_shaped(tag: i64) -> bool {
    tag == AnySlotTag::Null as i64
        || tag == AnySlotTag::Bool as i64
        || tag == AnySlotTag::I64 as i64
}

/// `-`, `*`, `/`, `%` on two Any-tagged `(tag, value)` pairs per
/// ES §13.6–§13.9. Both operands `ToNumber`-ed then the arithmetic
/// happens in IEEE 754. Result is boxed as either I64 (integer
/// fast-path; see [`ArithOp::allows_i64_fast_path`] +
/// [`tag_is_i64_shaped`]) or F64.
///
/// Out-of-range `op` → NaN-boxed (defensive; IR should never emit
/// this).
///
/// # Safety
///
/// If either `tag` is Heap, the corresponding `value` must be
/// null or a valid `*mut HeapHeader` — propagated through
/// [`any_to_number`].
pub(crate) unsafe fn any_arith(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    let arith_op = match ArithOp::from_i64(op) {
        Some(o) => o,
        // Defensive — match the C `default: NaN` branch.
        None => return alloc_number_f64(f64::NAN),
    };
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    let result = arith_op.apply(l, r);

    if arith_op.allows_i64_fast_path()
        && tag_is_i64_shaped(lt)
        && tag_is_i64_shaped(rt)
        && result >= i64::MIN as f64
        && result <= i64::MAX as f64
    {
        let int_result = result as i64;
        // Round-trip check: only box as I64 if the cast is lossless.
        if (int_result as f64) == result {
            return alloc_number_i64(int_result);
        }
    }
    alloc_number_f64(result)
}

/// Box an f64 into a fresh AnyBox tagged F64. Matches the C ABI's
/// `__torajs_any_box(ANY_F64, bitcast(f64).i64)` pattern. Shared
/// helper because any_arith has two callsites for it (defensive
/// NaN return + main F64 path) and any_add reuses it.
#[inline]
fn alloc_number_f64(value: f64) -> *mut c_void {
    AnyBox::alloc(AnySlotTag::F64, value.to_bits() as i64).as_ptr() as *mut c_void
}

/// Box an i64 into a fresh AnyBox tagged I64. Shared by every
/// integer-fast-path callsite (any_arith + any_add).
#[inline]
fn alloc_number_i64(value: i64) -> *mut c_void {
    AnyBox::alloc(AnySlotTag::I64, value).as_ptr() as *mut c_void
}

// ============================================================
// Addition (`+`) dispatch (JS spec §13.15.3 ApplyStringOrNumeric
// BinaryOperator)
// ============================================================

/// `+` on two Any-tagged `(tag, value)` pairs per ES §13.15.3.
/// If either operand is `Heap` + [`Tag::Str`] the result is the
/// String concatenation of both operands' `ToString`s. Otherwise
/// both operands go through ToNumber and the f64 sum is boxed —
/// I64 when both inputs are i64-shaped (Null/Bool/I64) AND the
/// sum round-trips through i64 losslessly, else F64.
///
/// Returns a fresh owned AnyBox (refcount = 1); caller drops.
///
/// # Safety
///
/// If either tag is `Heap`, the corresponding value must be null
/// or a valid `*mut HeapHeader` — propagated through both the
/// Str-path (where C-side `__torajs_str_concat` reads the Str
/// layout) and the numeric path (via [`any_to_number`]).
pub(crate) unsafe fn any_add(lt: i64, lv: i64, rt: i64, rv: i64) -> *mut c_void {
    // SAFETY: caller invariant — propagated.
    let l_is_str = unsafe { is_heap_str(lt, lv) };
    let r_is_str = unsafe { is_heap_str(rt, rv) };

    // String concatenation path (ES §13.15.3 — either side String
    // wins). Both operands go through ToString; the two
    // intermediates are dropped after the concat owns its own
    // copy of the bytes.
    if l_is_str || r_is_str {
        // SAFETY: any_to_str preserves the Heap-payload Safety
        // contract; result is a freshly-owned Str (refcount=1).
        let l_str = unsafe { any_to_str(lt, lv) };
        let r_str = unsafe { any_to_str(rt, rv) };
        // SAFETY: both pointers are freshly-owned Strs whose layout
        // begins with HeapHeader. __torajs_str_concat reads the
        // layout, allocates a new Str, returns ownership to us.
        let concat = unsafe { __torajs_str_concat(l_str as *const u8, r_str as *const u8) };
        // SAFETY: both Strs were rc=1 from any_to_str; rc_dec to 0
        // frees them.
        unsafe {
            __torajs_str_drop(l_str);
            __torajs_str_drop(r_str);
        }
        return AnyBox::alloc(AnySlotTag::Heap, concat as i64).as_ptr() as *mut c_void;
    }

    // Numeric path. ToNumber reuses the per-tag dispatch from
    // P2.3-d.1; same predicates as any_arith for the I64 fast-
    // path (i64-shaped tags + lossless f64↔i64 round-trip).
    //
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    let sum = l + r;

    if tag_is_i64_shaped(lt)
        && tag_is_i64_shaped(rt)
        && sum >= i64::MIN as f64
        && sum <= i64::MAX as f64
    {
        let int_sum = sum as i64;
        if (int_sum as f64) == sum {
            return alloc_number_i64(int_sum);
        }
    }
    alloc_number_f64(sum)
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    // Test binary needs both extern "C" symbols torajs-anyvalue
    // declares: torajs-rc's __torajs_weakref_target_dying (from
    // rc_dec's hit-zero hook) AND `__torajs_value_drop_heap`
    // (called from AnyBox::drop_owned for Heap-tagged children).
    //
    // The real `__torajs_value_drop_heap` lives in
    // `torajs_rc::drop_dispatch` (P7.i-drop, 2026-05-24); the
    // shipped binary resolves through libtorajs_rc.a. cargo test
    // for this crate links torajs-rc's rlib but Rust DCE strips
    // the dispatch fn since no Rust call site references it — the
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

    #[test]
    fn anybox_layout_matches_c_definition() {
        // C side: 24 bytes total, header @0, tag @8, value @16.
        // Drift here would break the const-offset arithmetic ssa_
        // lower emits at every dynobj / Array<Any> read/write.
        assert_eq!(size_of::<AnyBox>(), 24);
        assert_eq!(align_of::<AnyBox>(), 8);
        assert_eq!(offset_of!(AnyBox, header), 0);
        assert_eq!(offset_of!(AnyBox, tag), 8);
        assert_eq!(offset_of!(AnyBox, value), 16);
    }

    #[test]
    fn alloc_inline_null_then_drop() {
        let p = AnyBox::alloc(AnySlotTag::Null, 0);
        // SAFETY: just-allocated, exclusive.
        unsafe {
            assert_eq!(p.as_ref().tag, 0);
            assert_eq!(p.as_ref().value, 0);
            assert_eq!(p.as_ref().header.refcount, 1);
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn alloc_bool_then_unbox() {
        let p = AnyBox::alloc(AnySlotTag::Bool, 1);
        unsafe {
            assert_eq!(__torajs_any_unbox_tag(p.as_ptr() as *const c_void), 1);
            assert_eq!(__torajs_any_unbox_value(p.as_ptr() as *const c_void), 1);
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn alloc_i64_then_read() {
        let p = AnyBox::alloc(AnySlotTag::I64, 42);
        unsafe {
            assert!(matches!(p.as_ref().read(), AnyValue::I64(42)));
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn alloc_f64_round_trips_through_bitcast() {
        let n: f64 = 3.14159;
        let p = AnyBox::alloc(AnySlotTag::F64, n.to_bits() as i64);
        unsafe {
            match p.as_ref().read() {
                AnyValue::F64(x) => assert_eq!(x.to_bits(), n.to_bits()),
                _ => panic!("expected F64"),
            }
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn alloc_heap_increments_child_rc() {
        let mut child = HeapHeader::new(Tag::Str);
        let child_ptr = &mut child as *mut HeapHeader;
        let initial_rc = child.refcount;

        let p = AnyBox::alloc(AnySlotTag::Heap, child_ptr as i64);
        // Heap-tagged alloc rc_inc's the child.
        assert_eq!(child.refcount, initial_rc + 1);

        // Drop the box (with our stubbed value_drop_heap, no-op).
        unsafe { AnyBox::drop_owned(p) };
        // Note: our test stub for `__torajs_value_drop_heap` is
        // a no-op so it doesn't actually rc_dec the child. The
        // production runtime resolves the real C symbol which
        // does the rc_dec. The assertion below verifies the
        // *box*'s drop ran (child rc was not double-touched
        // here).
        assert_eq!(child.refcount, initial_rc + 1);
    }

    #[test]
    fn alloc_undef_round_trips() {
        let p = AnyBox::alloc(AnySlotTag::Undef, 0);
        unsafe {
            assert_eq!(__torajs_any_unbox_tag(p.as_ptr() as *const c_void), 5);
            assert!(matches!(p.as_ref().read(), AnyValue::Undef));
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn ffi_box_unbox_tag_value_round_trip() {
        unsafe {
            let p = __torajs_any_box(2 /* I64 */, 12345);
            assert_eq!(__torajs_any_unbox_tag(p), 2);
            assert_eq!(__torajs_any_unbox_value(p), 12345);
            __torajs_any_box_drop(p);
        }
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

    #[test]
    fn drop_owned_static_literal_is_no_op() {
        // Build a box with the STATIC_LITERAL flag pre-set; drop
        // should bail before rc_dec runs.
        let p = AnyBox::alloc(AnySlotTag::I64, 99);
        unsafe {
            (*p.as_ptr()).header.flags |= torajs_rc::FLAG_STATIC_LITERAL;
            // Save count snapshot.
            let count_before = (*p.as_ptr()).header.refcount;
            AnyBox::drop_owned(p);
            // refcount untouched; allocation NOT freed (we leak
            // it intentionally — STATIC_LITERAL boxes are program-
            // lifetime).
            assert_eq!((*p.as_ptr()).header.refcount, count_before);
            // Manually clear flag + drop for the test to not leak.
            (*p.as_ptr()).header.flags &= !torajs_rc::FLAG_STATIC_LITERAL;
            AnyBox::drop_owned(p);
        }
    }

    #[test]
    fn ffi_drop_null_is_safe() {
        unsafe {
            __torajs_any_box_drop(std::ptr::null_mut());
        }
    }

    #[test]
    fn ffi_box_unknown_tag_falls_back_to_null() {
        // Defensive: IR shouldn't emit invalid tags but the FFI
        // shim treats them as Null.
        unsafe {
            let p = __torajs_any_box(99, 0);
            assert_eq!(__torajs_any_unbox_tag(p), 0);
            __torajs_any_box_drop(p);
        }
    }

    // ---- P2.3-b: strict equality ----

    #[test]
    fn anyvalue_strict_eq_null_undef() {
        assert!(AnyValue::Null.strict_eq(AnyValue::Null));
        assert!(AnyValue::Undef.strict_eq(AnyValue::Undef));
        // Cross-tag: null vs undefined are NOT strict-eq per
        // ES §7.2.13.
        assert!(!AnyValue::Null.strict_eq(AnyValue::Undef));
        assert!(!AnyValue::Undef.strict_eq(AnyValue::Null));
    }

    #[test]
    fn anyvalue_strict_eq_bool_i64() {
        assert!(AnyValue::Bool(true).strict_eq(AnyValue::Bool(true)));
        assert!(AnyValue::Bool(false).strict_eq(AnyValue::Bool(false)));
        assert!(!AnyValue::Bool(true).strict_eq(AnyValue::Bool(false)));
        assert!(AnyValue::I64(42).strict_eq(AnyValue::I64(42)));
        assert!(!AnyValue::I64(42).strict_eq(AnyValue::I64(43)));
        // Cross-tag: bool vs int are NOT strict-eq even if values
        // could coerce.
        assert!(!AnyValue::Bool(true).strict_eq(AnyValue::I64(1)));
    }

    #[test]
    fn anyvalue_strict_eq_f64_ieee_semantics() {
        // NaN !== NaN per IEEE 754.
        assert!(!AnyValue::F64(f64::NAN).strict_eq(AnyValue::F64(f64::NAN)));
        // +0.0 === -0.0 per IEEE 754.
        assert!(AnyValue::F64(0.0).strict_eq(AnyValue::F64(-0.0)));
        assert!(AnyValue::F64(1.5).strict_eq(AnyValue::F64(1.5)));
        assert!(!AnyValue::F64(1.5).strict_eq(AnyValue::F64(2.5)));
        // Infinity equals itself.
        assert!(AnyValue::F64(f64::INFINITY).strict_eq(AnyValue::F64(f64::INFINITY)));
    }

    #[test]
    fn anyvalue_strict_eq_heap_pointer_identity() {
        let mut h1 = HeapHeader::new(Tag::Obj);
        let mut h2 = HeapHeader::new(Tag::Obj);
        let p1 = NonNull::new(&mut h1 as *mut HeapHeader);
        let p2 = NonNull::new(&mut h2 as *mut HeapHeader);
        assert!(AnyValue::Heap(p1).strict_eq(AnyValue::Heap(p1)));
        // Different addresses, both Tag::Obj (non-Str) → false.
        assert!(!AnyValue::Heap(p1).strict_eq(AnyValue::Heap(p2)));
        // Both none → true (null === null on the heap side).
        assert!(AnyValue::Heap(None).strict_eq(AnyValue::Heap(None)));
        // One null, one not → false.
        assert!(!AnyValue::Heap(None).strict_eq(AnyValue::Heap(p1)));
    }

    #[test]
    fn anyvalue_strict_eq_str_via_str_eq() {
        // Two Str-tagged headers at the same address — stub
        // __torajs_str_eq returns 1 on pointer identity, so this
        // is true via the byte-equality fast path.
        let mut s = HeapHeader::new(Tag::Str);
        let p = NonNull::new(&mut s as *mut HeapHeader);
        assert!(AnyValue::Heap(p).strict_eq(AnyValue::Heap(p)));
    }

    #[test]
    fn ffi_any_any_strict_eq_round_trip() {
        unsafe {
            // Both null → true.
            assert!(__torajs_any_any_strict_eq(
                core::ptr::null(),
                core::ptr::null()
            ));
            // Same-tag same-value box pair → true.
            let p1 = __torajs_any_box(2 /* I64 */, 42);
            let p2 = __torajs_any_box(2, 42);
            assert!(__torajs_any_any_strict_eq(p1, p2));
            // Same-tag different-value → false.
            let p3 = __torajs_any_box(2, 43);
            assert!(!__torajs_any_any_strict_eq(p1, p3));
            // Different tag → false.
            let p4 = __torajs_any_box(1 /* Bool */, 1);
            assert!(!__torajs_any_any_strict_eq(p1, p4));
            __torajs_any_box_drop(p1);
            __torajs_any_box_drop(p2);
            __torajs_any_box_drop(p3);
            __torajs_any_box_drop(p4);
        }
    }

    // ---- P2.3-d.1: ToNumber coercion ----

    #[test]
    fn anyvalue_to_number_inline_tags() {
        assert_eq!(AnyValue::Null.to_number(), 0.0);
        assert!(AnyValue::Undef.to_number().is_nan());
        assert_eq!(AnyValue::Bool(true).to_number(), 1.0);
        assert_eq!(AnyValue::Bool(false).to_number(), 0.0);
        assert_eq!(AnyValue::I64(0).to_number(), 0.0);
        assert_eq!(AnyValue::I64(42).to_number(), 42.0);
        assert_eq!(AnyValue::I64(-7).to_number(), -7.0);
        assert_eq!(AnyValue::F64(3.14).to_number(), 3.14);
        // F64 NaN propagates.
        assert!(AnyValue::F64(f64::NAN).to_number().is_nan());
        // Unknown defensively → NaN.
        assert!(AnyValue::Unknown.to_number().is_nan());
    }

    #[test]
    fn anyvalue_to_number_heap_null_is_zero() {
        // Heap(None) is the "tag=Heap, value=NULL" case — distinct
        // from AnyValue::Null tag-wise. ToNumber here matches the
        // C ABI: 0.0 (defensive, not NaN).
        assert_eq!(AnyValue::Heap(None).to_number(), 0.0);
    }

    #[test]
    fn anyvalue_to_number_heap_str_delegates_to_str_to_number() {
        // Heap+Str → __torajs_str_to_number; test stub returns 42.0.
        let mut s = HeapHeader::new(Tag::Str);
        let p = NonNull::new(&mut s as *mut HeapHeader);
        assert_eq!(AnyValue::Heap(p).to_number(), 42.0);
    }

    #[test]
    fn anyvalue_to_number_heap_non_str_is_nan() {
        // Heap+Obj (or any non-Str) → NaN, matches the C ABI's
        // "objects coerce to NaN" path (pre-valueOf-method era).
        let mut h = HeapHeader::new(Tag::Obj);
        let p = NonNull::new(&mut h as *mut HeapHeader);
        assert!(AnyValue::Heap(p).to_number().is_nan());
    }

    #[test]
    fn ffi_any_to_number_round_trip_inline() {
        unsafe {
            // Null box → 0.0.
            let p_null = __torajs_any_box(0, 0);
            assert_eq!(__torajs_any_to_number(p_null), 0.0);
            __torajs_any_box_drop(p_null);

            // Undef box → NaN.
            let p_undef = __torajs_any_box(5, 0);
            assert!(__torajs_any_to_number(p_undef).is_nan());
            __torajs_any_box_drop(p_undef);

            // Bool(true) → 1.0; Bool(false) → 0.0.
            let p_t = __torajs_any_box(1, 1);
            let p_f = __torajs_any_box(1, 0);
            assert_eq!(__torajs_any_to_number(p_t), 1.0);
            assert_eq!(__torajs_any_to_number(p_f), 0.0);
            __torajs_any_box_drop(p_t);
            __torajs_any_box_drop(p_f);

            // I64(123) → 123.0.
            let p_i = __torajs_any_box(2, 123);
            assert_eq!(__torajs_any_to_number(p_i), 123.0);
            __torajs_any_box_drop(p_i);

            // F64(2.5) → 2.5.
            let bits = 2.5_f64.to_bits() as i64;
            let p_f64 = __torajs_any_box(3, bits);
            assert_eq!(__torajs_any_to_number(p_f64), 2.5);
            __torajs_any_box_drop(p_f64);

            // Null pointer box → 0.0 (defensive).
            assert_eq!(__torajs_any_to_number(core::ptr::null()), 0.0);
        }
    }

    #[test]
    fn ffi_any_to_number_inner_packed_pair() {
        unsafe {
            // Each tag variant via the packed-pair entry point.
            assert_eq!(__torajs_any_to_number_inner(0 /* Null */, 0), 0.0);
            assert!(__torajs_any_to_number_inner(5 /* Undef */, 0).is_nan());
            assert_eq!(__torajs_any_to_number_inner(1 /* Bool */, 1), 1.0);
            assert_eq!(__torajs_any_to_number_inner(1 /* Bool */, 0), 0.0);
            assert_eq!(__torajs_any_to_number_inner(2 /* I64  */, 99), 99.0);
            let bits = (-3.5_f64).to_bits() as i64;
            assert_eq!(__torajs_any_to_number_inner(3 /* F64  */, bits), -3.5);
            // Heap-null → 0.0 (defensive C-ABI parity).
            assert_eq!(__torajs_any_to_number_inner(4 /* Heap */, 0), 0.0);
            // Unknown tag → NaN.
            assert!(__torajs_any_to_number_inner(99, 0).is_nan());
        }
    }

    #[test]
    fn ffi_any_to_number_inner_heap_str_delegates() {
        // Heap-tagged Str via the inner shim → test stub returns 42.0.
        let mut s = HeapHeader::new(Tag::Str);
        unsafe {
            let p = &mut s as *mut HeapHeader as i64;
            assert_eq!(__torajs_any_to_number_inner(4, p), 42.0);
        }
    }

    #[test]
    fn ffi_any_strict_eq_box_vs_concrete() {
        unsafe {
            // Null box vs Null tag → true.
            assert!(__torajs_any_strict_eq(core::ptr::null(), 0, 0));
            // Null box vs Undef tag → false.
            assert!(!__torajs_any_strict_eq(core::ptr::null(), 5, 0));
            // I64 box vs same I64 → true.
            let p = __torajs_any_box(2, 42);
            assert!(__torajs_any_strict_eq(p, 2, 42));
            assert!(!__torajs_any_strict_eq(p, 2, 43));
            assert!(!__torajs_any_strict_eq(p, 3 /* F64 */, 42));
            __torajs_any_box_drop(p);
        }
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
    fn ffi_any_compare_round_trip() {
        unsafe {
            // Quick round-trip via the FFI shim — exact same args
            // ssa_lower emits.
            assert!(__torajs_any_compare(0, 2, 1, 2, 2));
            assert!(!__torajs_any_compare(2, 2, 1, 2, 2));
            // Heap-Str pair via the FFI entry point.
            let (_a, pa) = make_str_blob(b"abc");
            let (_b, pb) = make_str_blob(b"abd");
            assert!(__torajs_any_compare(0, 4, pa as i64, 4, pb as i64));
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

    /// Unbox a fresh AnyBox returned by any_arith into a typed
    /// view, then drop the box. Used in every arith test to assert
    /// both the tag and value the dispatcher chose.
    unsafe fn unbox_drop(p: *mut c_void) -> AnyValue {
        let b = unsafe { &*(p as *const AnyBox) };
        let view = b.read();
        unsafe { __torajs_any_box_drop(p) };
        view
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
            assert!(matches!(unbox_drop(p), AnyValue::I64(7)));
            // 4 * 5 = 20 → I64.
            let p = any_arith(1, 2, 4, 2, 5);
            assert!(matches!(unbox_drop(p), AnyValue::I64(20)));
            // 10 % 3 = 1 → I64.
            let p = any_arith(3, 2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyValue::I64(1)));
        }
    }

    #[test]
    fn any_arith_div_always_returns_f64() {
        unsafe {
            // 10 / 4 = 2.5 → F64 (fractional).
            let p = any_arith(2, 2, 10, 2, 4);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x == 2.5));
            // 10 / 5 = 2 → still F64 (Div opts out of integer fast-path).
            let p = any_arith(2, 2, 10, 2, 5);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x == 2.0));
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
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x == 6.0));
        }
    }

    #[test]
    fn any_arith_bool_null_treated_as_i64_shaped() {
        unsafe {
            // true + true (Mul) — both Bool-tagged → I64 fast-path.
            let p = any_arith(1, 1 /* Bool */, 1, 1 /* Bool */, 1);
            assert!(matches!(unbox_drop(p), AnyValue::I64(1)));
            // null - null = 0 → I64.
            let p = any_arith(0, 0 /* Null */, 0, 0 /* Null */, 0);
            assert!(matches!(unbox_drop(p), AnyValue::I64(0)));
        }
    }

    #[test]
    fn any_arith_undef_propagates_nan_as_f64() {
        unsafe {
            // undefined * 2 → NaN → F64 (NaN never round-trips through i64).
            let p = any_arith(1, 5 /* Undef */, 0, 2 /* I64 */, 2);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x.is_nan()));
        }
    }

    #[test]
    fn any_arith_unknown_op_returns_nan_f64() {
        unsafe {
            // op=99 — defensive NaN-box.
            let p = any_arith(99, 2, 1, 2, 2);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x.is_nan()));
        }
    }

    #[test]
    fn any_arith_integer_fractional_result_uses_f64() {
        unsafe {
            // I64(1) % I64 doesn't happen here, but I64-1 + I64-1 should
            // be I64. Verify that integer result via Mod that lands on an
            // exact integer DOES use I64.
            let p = any_arith(3 /* Mod */, 2, 17, 2, 5); // 17 % 5 = 2
            assert!(matches!(unbox_drop(p), AnyValue::I64(2)));
        }
    }

    #[test]
    fn ffi_any_arith_round_trip() {
        unsafe {
            // FFI smoke test — Sub via the public symbol.
            let p = __torajs_any_arith(0, 2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyValue::I64(7)));
        }
    }

    // ---- P2.3-d.4: addition (`+`) dispatch ----

    #[test]
    fn any_add_i64_plus_i64_returns_i64() {
        unsafe {
            // 10 + 3 → I64.
            let p = any_add(2, 10, 2, 3);
            assert!(matches!(unbox_drop(p), AnyValue::I64(13)));
            // Negative result.
            let p = any_add(2, 3, 2, -10);
            assert!(matches!(unbox_drop(p), AnyValue::I64(-7)));
            // Zero result.
            let p = any_add(2, 5, 2, -5);
            assert!(matches!(unbox_drop(p), AnyValue::I64(0)));
        }
    }

    #[test]
    fn any_add_bool_null_treated_as_i64_shaped() {
        unsafe {
            // true + 1 → 2 (I64). Both Bool/I64 are i64-shaped.
            let p = any_add(1 /* Bool */, 1, 2 /* I64 */, 1);
            assert!(matches!(unbox_drop(p), AnyValue::I64(2)));
            // null + null → 0 (I64).
            let p = any_add(0 /* Null */, 0, 0 /* Null */, 0);
            assert!(matches!(unbox_drop(p), AnyValue::I64(0)));
            // false + true → 1 (I64).
            let p = any_add(1 /* Bool */, 0, 1 /* Bool */, 1);
            assert!(matches!(unbox_drop(p), AnyValue::I64(1)));
        }
    }

    #[test]
    fn any_add_f64_input_forces_f64() {
        unsafe {
            // F64 + I64 → F64 even if sum is integer-valued.
            let two_bits = 2.0_f64.to_bits() as i64;
            let p = any_add(3 /* F64 */, two_bits, 2 /* I64 */, 3);
            // 2.0 + 3 = 5.0, but F64-tagged input opts out of I64 fast-path.
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x == 5.0));
        }
    }

    #[test]
    fn any_add_fractional_result_uses_f64() {
        unsafe {
            // F64 1.5 + I64 2 → 3.5 (F64).
            let one_half_bits = 1.5_f64.to_bits() as i64;
            let p = any_add(3, one_half_bits, 2, 2);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x == 3.5));
        }
    }

    #[test]
    fn any_add_undef_propagates_nan() {
        unsafe {
            // undefined + 1 → NaN (Undef toNumber = NaN; any +NaN = NaN).
            let p = any_add(5 /* Undef */, 0, 2 /* I64 */, 1);
            assert!(matches!(unbox_drop(p), AnyValue::F64(x) if x.is_nan()));
        }
    }

    #[test]
    fn ffi_any_add_round_trip() {
        unsafe {
            // FFI smoke test — Sub via the public symbol.
            let p = __torajs_any_add(2, 7, 2, 5);
            assert!(matches!(unbox_drop(p), AnyValue::I64(12)));
        }
    }

    // (Str-concat path verified end-to-end via bun-parity fixture +
    // conformance — providing fully-functional stubs for the entire
    // any_to_str + str_concat chain would mean re-implementing the
    // Layer-2 str runtime here; not worth the complexity.)
}
