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
        let ptr = NonNull::new(raw).expect("AnyBox alloc OOM");
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
unsafe fn any_to_str(tag: i64, value: i64) -> *mut c_void {
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
fn payload_eq(tag: i64, lv: i64, rv: i64) -> bool {
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
// FFI shims — thin wrappers for ssa_lower-emitted IR
// ============================================================

/// FFI bridge to [`AnyBox::alloc`]. `tag` accepts the same `i64`
/// range as [`AnySlotTag`] discriminants; out-of-range tags fall
/// back to `Null` (defensive — IR shouldn't emit these).
///
/// # Safety
///
/// For `tag == AnySlotTag::Heap as i64`, `value` must be either
/// null or a valid `*mut HeapHeader` (the new box gains an owning
/// ref via `rc_inc`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box(tag: i64, value: i64) -> *mut c_void {
    let slot = match tag {
        0 => AnySlotTag::Null,
        1 => AnySlotTag::Bool,
        2 => AnySlotTag::I64,
        3 => AnySlotTag::F64,
        4 => AnySlotTag::Heap,
        5 => AnySlotTag::Undef,
        _ => AnySlotTag::Null,
    };
    AnyBox::alloc(slot, value).as_ptr() as *mut c_void
}

/// FFI bridge — read the boxed payload's tag.
///
/// # Safety
///
/// `box_ptr` must be a valid `*const AnyBox` (i.e. previously
/// returned by [`__torajs_any_box`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_tag(box_ptr: *const c_void) -> i64 {
    // SAFETY: caller invariant.
    unsafe { (*(box_ptr as *const AnyBox)).tag }
}

/// FFI bridge — read the boxed payload's raw value.
///
/// # Safety
///
/// `box_ptr` must be a valid `*const AnyBox`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_unbox_value(box_ptr: *const c_void) -> i64 {
    // SAFETY: caller invariant.
    unsafe { (*(box_ptr as *const AnyBox)).value }
}

/// FFI bridge to [`payload_rc_inc`]. Bumps the heap child rc
/// for `Heap`-tagged pairs; no-op otherwise.
///
/// # Safety
///
/// If `tag == Heap`, `value` must be null or a valid `*mut
/// HeapHeader`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_payload_rc_inc(tag: i64, value: i64) {
    payload_rc_inc(tag, value);
}

/// FFI bridge to [`AnyBox::drop_owned`]. Null-safe.
///
/// # Safety
///
/// `box_ptr` is null OR a valid `*mut AnyBox` previously returned
/// by [`__torajs_any_box`]; caller exclusively owns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box_drop(box_ptr: *mut c_void) {
    if let Some(p) = NonNull::new(box_ptr as *mut AnyBox) {
        // SAFETY: caller invariant.
        unsafe { AnyBox::drop_owned(p) };
    }
}

/// FFI bridge — Any === Any strict equality (JS spec §7.2.13).
///
/// # Safety
///
/// `l` and `r` are each null OR a valid `*const AnyBox`. Two-null
/// is true, one-null is false.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_any_strict_eq(l: *const c_void, r: *const c_void) -> bool {
    match (l.is_null(), r.is_null()) {
        (true, true) => true,
        (true, _) | (_, true) => false,
        _ => {
            // SAFETY: both ptrs non-null per the match arm.
            let lb = unsafe { &*(l as *const AnyBox) };
            let rb = unsafe { &*(r as *const AnyBox) };
            if lb.tag != rb.tag {
                return false;
            }
            payload_eq(lb.tag, lb.value, rb.value)
        }
    }
}

/// FFI bridge to [`any_to_str`]. Returns a freshly-owned `Str`
/// pointer the caller must drop. Used by ssa_lower at every
/// implicit ToString site (template literals, `+` mixing string
/// and non-string operands, `console.log(any)` printing, …).
///
/// # Safety
///
/// For `tag == Heap`, `value` is null or a valid `*mut
/// HeapHeader` pointing to a live heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_to_str(tag: i64, value: i64) -> *mut c_void {
    unsafe { any_to_str(tag, value) }
}

/// FFI bridge — Any === concrete (SSA-emitted `(tag, value)` pair
/// vs a box). Avoids a fresh box alloc per compare site.
///
/// `box_ptr == null` matches `rhs_tag == AnySlotTag::Null` and
/// nothing else.
///
/// # Safety
///
/// `box_ptr` is null OR a valid `*const AnyBox`. `rhs_tag` is a
/// well-formed [`AnySlotTag`] discriminant; `rhs_value` is the
/// packing the SSA layer chose (bitcast for f64, zext for bool,
/// raw cast for i64, pointer-as-i64 for heap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_strict_eq(
    box_ptr: *const c_void,
    rhs_tag: i64,
    rhs_value: i64,
) -> bool {
    if box_ptr.is_null() {
        return rhs_tag == AnySlotTag::Null as i64;
    }
    // SAFETY: non-null per the early return.
    let b = unsafe { &*(box_ptr as *const AnyBox) };
    if b.tag != rhs_tag {
        return false;
    }
    payload_eq(b.tag, b.value, rhs_value)
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
    // rc_dec's hit-zero hook) AND runtime_str.c's
    // __torajs_value_drop_heap (from AnyBox::drop_owned). In the
    // shipped binary both are resolved by the C runtime; in tests
    // we no-op them.
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
}
