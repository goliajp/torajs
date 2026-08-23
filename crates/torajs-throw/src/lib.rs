//! Catchable-throw infrastructure for the torajs AOT TypeScript
//! runtime — slot machinery + native-error factory registry.
//!
//! Layer-1 substrate (no upstream deps). Companion to `torajs-rc`
//! + `torajs-anyvalue` in P2 of the architecture rewrite (see
//! `docs/architecture-rewrite.md`). Replaces the C-side native-
//! error registry + `__torajs_throw_range_error` / `__torajs_throw
//! _type_error` wrappers in the pre-rewrite `runtime_str.c`.
//!
//! ## What this crate provides
//!
//! 1. **Native-error factory registry** ([`registry`]) — one slot per
//!    runtime-buildable Error class, into which
//!    `synthesize_module_init` registers that class's `__new_<C>`
//!    factory. When a runtime helper raises a native error (e.g.
//!    bigint div-by-zero), the registry is consulted to build a real
//!    catchable instance (with proper `.message` / `.name` /
//!    `instanceof` / `.stack`) instead of the legacy bare-string
//!    fallback. `Promise.any`'s AggregateError is built through the
//!    same table without being thrown at all.
//!
//! 2. **`throw_range_error` / `throw_type_error` helpers** —
//!    cross-translation-unit shims that bigint / regex / dynobj
//!    helpers call to raise catchable spec-mandated errors. Each
//!    allocates a Str holding the message, invokes the registered
//!    factory (or falls back to a bare-string throw), and stores
//!    the result into the thread-local throw slot via the still-
//!    LLVM-IR-emitted `__torajs_throw_set` (P2.4-b ports that to
//!    Rust too).
//!
//! ## Design notes (per project "石头 + 水泥" metaphor)
//!
//! This is a stone: a self-contained Layer-1 substrate other crates
//! depend on. The registry is an `AtomicPtr<()>` array —
//! single-write-at-startup, read-only after — `AtomicPtr` only for
//! Rust's safety story, NOT for actual concurrent mutation (the
//! runtime is single-threaded).
//!
//! The Str-allocation + bytes-write delegate to the C-side
//! `__torajs_str_alloc_pooled` helper (Layer-2 `torajs-str` rewrite
//! ports those later); the throw-slot write delegates to the
//! LLVM-IR-emitted `__torajs_throw_set` (P2.4-b moves it to Rust).
//!
//! ## Why not `static mut`
//!
//! `static mut` is being deprecated in Rust 2024. `AtomicPtr<()>`
//! is the idiomatic replacement — each slot is independently
//! load / store with `Relaxed` ordering (no other state depends on
//! happens-before with these stores, since the registration phase
//! completes before any code paths can throw).
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as [`torajs-rc`] and [`torajs-anyvalue`]: cargo's
//! `cargo test` + dual `crate-type = ["rlib", "staticlib"]` +
//! `no_std` combination triggers a precompiled-core panic-strategy
//! mismatch (the test runner forces unwind panics, precompiled
//! core demands abort) that has no clean fix on stable. `std`
//! staticlibs link cleanly at `tr build` time — cc + LLVM-LTO
//! tolerates std symbol overlap between Rust-emitted .a's.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================
// Native-error factory registry (see `registry`)
// ============================================================

pub mod registry;

pub use registry::{
    __torajs_make_aggregate_error, __torajs_register_native_error, AggregateErrorFactory,
    NativeErrorFactory, SLOT_AGGREGATE_ERROR, SLOT_ERROR, SLOT_RANGE_ERROR, SLOT_REFERENCE_ERROR,
    SLOT_SYNTAX_ERROR, SLOT_TYPE_ERROR, SLOT_URI_ERROR,
};
use registry::{ANY_VALUE_UNDEFINED, SLOT_COUNT, lookup_factory};

// ============================================================
// External Str helpers (still in C; Layer-2 `torajs-str` rewrite
// ports them later)
// ============================================================

unsafe extern "C" {
    /// Allocate a Str with `len` bytes of payload capacity; the
    /// returned ptr's `[header:8][len:u64@8][bytes:len@16]` layout
    /// is pre-initialized except for the bytes (caller writes
    /// those at `*+ 16`). Implemented in `torajs-str` (`block.rs`)
    /// since the Layer-2 rewrite.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    /// libc `strlen` — Layer-0 system primitive; no `dep` cost.
    fn strlen(s: *const c_char) -> usize;

    /// Release one refcount on a Str block (static-literal and
    /// Substr-view aware; frees via the pool when rc hits 0).
    /// Implemented in `torajs-str` (`str_drop.rs`).
    fn __torajs_str_drop(s: *mut u8);
}

// ============================================================
// Throw-slot machinery (LLVM-IR-emitted → Rust statics)
// ============================================================

/// Process-global "is a throw in flight?" flag. Set to 1 by
/// [`__torajs_throw_set`]; cleared back to 0 by
/// [`__torajs_throw_take`]. The runtime is single-threaded so a
/// relaxed atomic load/store is sufficient — the AtomicI64 wrapper
/// exists for Rust's safety story (no `static mut`), not for actual
/// concurrency.
///
/// Exported under a C name on purpose: user code reads it INLINE
/// (`ssa_lower_emit_throw_check` emits `GlobalRef` + `Load` against
/// `___torajs_throw_active`) instead of calling
/// [`__torajs_throw_check`]. A throw check sits after every call
/// that may raise, so on a hot loop the call itself was the cost —
/// rotation 470 measured it at ~10% of `class-method` (391 of ~3700
/// leaf samples). The function stays for runtime-side callers.
#[unsafe(no_mangle)]
pub static __torajs_throw_active: AtomicI64 = AtomicI64::new(0);
pub(crate) use __torajs_throw_active as THROW_ACTIVE;

/// Dynamic tag of the in-flight throw value. `AnySlotTag` discrim
/// (0=Null, 1=Bool, 2=I64, 3=F64, 4=Heap, 5=Undef). Catch sites
/// with `: any` annotation read this via [`__torajs_throw_take_tag`]
/// to reconstruct the boxed Any; typed `: T` catches ignore it.
pub(crate) static THROW_TAG: AtomicI64 = AtomicI64::new(0);

/// Packed i64 payload of the in-flight throw. Bitcast from f64
/// for F64 tag; raw cast from i64 for I64 tag; cast from
/// `*mut Heap` for Heap tag. ssa_lower-emitted code reads it via
/// [`__torajs_throw_take`].
pub(crate) static THROW_VALUE: AtomicI64 = AtomicI64::new(0);

/// Store `(tag, value)` into the throw slot and flag it active.
/// Public FFI replacing ssa_inkwell's `define_throw_set` LLVM-IR
/// emit (P2.4-b: that path is now gone).
///
/// # Safety
///
/// No Rust-side invariants — `tag` and `value` are opaque i64s.
/// The caller (ssa_lower-emitted code, cross-TU callers like
/// torajs-promise's `__torajs_throw_set(...)` sites) chose the
/// encoding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_set(tag: i64, value: i64) {
    // Order matters at LLVM-IR level (other paths peek tag+value
    // after seeing active=1) so we set tag/value first, active
    // last. Relaxed ordering: single-threaded runtime — there's
    // no concurrent reader needing happens-before.
    THROW_TAG.store(tag, Ordering::Relaxed);
    THROW_VALUE.store(value, Ordering::Relaxed);
    THROW_ACTIVE.store(1, Ordering::Relaxed);
}

/// Read the `active` flag — non-zero iff a throw is in flight.
/// Used by ssa_lower's `emit_throw_check` after every runtime-
/// intrinsic call that may raise (bigint div-by-zero, dynobj
/// frozen-set, etc.).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_check() -> i64 {
    THROW_ACTIVE.load(Ordering::Relaxed)
}

/// Read the throw value and clear the `active` flag. Called by
/// the user-fn's catch block (or fn boundary propagation) to
/// consume the throw — pairs with `__torajs_throw_take_tag` when
/// the catch is `: any`-typed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_take() -> i64 {
    let v = THROW_VALUE.load(Ordering::Relaxed);
    // Side effect: clear active. Tag + value stay (catch-side
    // take_tag may run after this).
    THROW_ACTIVE.store(0, Ordering::Relaxed);
    v
}

/// Peek the throw tag without clearing active. Called by `: any`-
/// typed catches BEFORE `__torajs_throw_take` so the dynamic tag
/// is captured (take clears active as a side effect but leaves
/// the tag slot untouched). Typed-tier catches skip this and
/// let the cast helper widen the i64 value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_take_tag() -> i64 {
    THROW_TAG.load(Ordering::Relaxed)
}

/// Byte offset of the Str payload within the heap layout
/// `[header:8][len:8][bytes:N]`. Mirror of the C
/// `__TORAJS_STR_HDR_SIZE` and `torajs-anyvalue`'s `STR_HDR_SIZE`.
pub(crate) const STR_HDR_SIZE: usize = 16;
/// Byte offset of the Str length field within the same layout.
pub(crate) const STR_LEN_OFF: usize = 8;

/// `tag` value matching `AnySlotTag::Heap` — refers to a heap-
/// allocated payload (here a Str or an Error subclass instance).
/// Hard-coded to match the C `__TORAJS_ANY_HEAP` constant and
/// `AnySlotTag::Heap as i64` from `torajs-rc`.
pub(crate) const ANY_TAG_HEAP: i64 = 4;

// ============================================================
// throw_native + range_error / type_error wrappers
// ============================================================

/// Raise a native error for the given slot:
/// - Allocate a Str holding the message.
/// - If a factory is registered for this slot, call it to build a
///   real Error-subclass instance, then `throw_set(HEAP, instance)`.
/// - Otherwise fall back to throwing the bare Str (legacy behavior
///   for unregistered slots — the call site's `emit_throw_check`
///   propagates either way).
///
/// `slot` accepts `0`/`1`/`2` (Error/TypeError/RangeError); out-
/// of-range values are silently treated as "unregistered" (bare-
/// string throw).
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership of `msg`; this function only reads its
/// bytes.
unsafe fn throw_native(slot: i64, msg: *const c_char) {
    // SAFETY: msg is a valid NUL-terminated C string per caller
    // invariant; strlen is libc-provided.
    let len = unsafe { strlen(msg) } as u64;
    // SAFETY: __torajs_str_alloc_pooled returns a Str whose header
    // is initialized + len-field set; we own the just-allocated
    // refcount=1.
    let err = unsafe { __torajs_str_alloc_pooled(len) };
    if len > 0 {
        // SAFETY: dst points at the payload offset (+STR_HDR_SIZE)
        // for `len` bytes; src is the C string the caller pinkied
        // is at least `len` bytes long; non-overlapping by virtue
        // of err being just-allocated.
        unsafe {
            ptr::copy_nonoverlapping(msg as *const u8, err.add(STR_HDR_SIZE), len as usize);
        }
    }
    // SAFETY: err is the just-minted Str we own.
    unsafe { throw_native_str(slot, err) };
}

/// Factory dispatch over an OWNED just-minted message Str `err` —
/// the tail of [`throw_native`], split out so raisers that build
/// their message from tr Str inputs (not C strings) can share it.
///
/// # Safety
///
/// `err` must be a valid, owned (rc = 1) Str block; ownership
/// transfers to this function.
unsafe fn throw_native_str(slot: i64, err: *mut u8) {
    // First throw wins. A kernel that records a throw can be called
    // again before the compiled caller's throw check runs — the
    // any-lane member probe's tag/value twin channels each invoke
    // the same accessor kernel back-to-back — and the second raise
    // here would run the error FACTORY (a codegen'd TS ctor) with
    // the pending throw still set, which makes its internal call
    // sites early-return and hands back null as the "instance".
    // Spec-wise the first abrupt completion already ended the
    // operation; a second record inside the same window is the same
    // completion re-announced, never a new one.
    if THROW_ACTIVE.load(Ordering::Relaxed) != 0 {
        // SAFETY: err is the caller's owned just-minted Str.
        unsafe { __torajs_str_drop(err) };
        return;
    }
    if slot >= 0 && (slot as usize) < SLOT_COUNT {
        if let Some(factory) = lookup_factory(slot as usize) {
            // SAFETY: factory is a valid NativeErrorFactory per the
            // safety contract of __torajs_register_native_error;
            // err is a valid freshly-allocated Str.
            let inst = unsafe { factory(err as *mut c_void, ANY_VALUE_UNDEFINED) };
            // The factory is a codegen'd TS-level `__new_<C>` fn:
            // its `message` param follows the standard borrow
            // convention and the ctor's `this.message = message`
            // field store takes its own retained reference (rc 1→2
            // measured). Release our mint stake — the instance's
            // field is now the sole owner. Without this every
            // native throw stranded one msg Str per call.
            // SAFETY: err is a valid non-view Str block; rc ≥ 1.
            unsafe { __torajs_str_drop(err) };
            // P2.4-b — direct call to the local Rust impl of
            // __torajs_throw_set (no extern hop). Same observable
            // semantics as the LLVM-IR-emitted version it replaces.
            unsafe { __torajs_throw_set(ANY_TAG_HEAP, inst as i64) };
            return;
        }
    }
    // Unregistered slot or out-of-range — bare-string fallback.
    unsafe { __torajs_throw_set(ANY_TAG_HEAP, err as i64) };
}

/// Cross-TU wrapper: torajs-bigint / torajs-regex / etc. call
/// this to raise a catchable `RangeError` (div-by-zero,
/// negative exponent, shift-too-large, `s.matchAll(re)` without
/// `g` flag, ...). The ssa_lower-side `emit_throw_check` after the
/// call propagates to the user's try/catch.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_range_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_RANGE_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `TypeError`. Parallel to
/// [`__torajs_throw_range_error`]; used by torajs-regex and
/// any future cross-TU caller raising a catchable TypeError.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_TYPE_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `ReferenceError` — RFC
/// 20260718-error-message-own-prop 刀 3 (derived-ctor no-super).
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_REFERENCE_ERROR as i64, msg) };
}

/// RFC 20260730-undeclared-ident — §6.2.5.5 GetValue on an
/// unresolvable Reference. `name` is the identifier as a tr Str
/// (the lowerer's interned literal); the message is
/// `<name> is not defined`, matching bun / V8 text.
///
/// # Safety
///
/// `name` must be a valid Str block pointer. Borrowed — only its
/// bytes are read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_reference_error_name(name: *mut c_void) {
    const SUFFIX: &[u8] = b" is not defined";
    let name = name as *const u8;
    // SAFETY: name is a valid Str block per caller invariant; len
    // field lives at STR_LEN_OFF.
    let name_len = unsafe { (name.add(STR_LEN_OFF) as *const u32).read() } as usize;
    let total = (name_len + SUFFIX.len()) as u64;
    // SAFETY: __torajs_str_alloc_pooled returns an initialized Str
    // header with the len field set; we own the rc = 1 allocation.
    let err = unsafe { __torajs_str_alloc_pooled(total) };
    // SAFETY: dst spans `total` bytes past the payload offset of the
    // just-allocated block; both srcs are readable for their lengths
    // and non-overlapping with the fresh allocation.
    unsafe {
        ptr::copy_nonoverlapping(name.add(STR_HDR_SIZE), err.add(STR_HDR_SIZE), name_len);
        ptr::copy_nonoverlapping(
            SUFFIX.as_ptr(),
            err.add(STR_HDR_SIZE + name_len),
            SUFFIX.len(),
        );
        throw_native_str(SLOT_REFERENCE_ERROR as i64, err);
    }
}

/// Cross-TU wrapper for `SyntaxError` — RFC 20260720 刀 5b (the
/// §7.1.14 StringToBigInt parse-failure raise in torajs-bigint).
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_syntax_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_SYNTAX_ERROR as i64, msg) };
}

/// Cross-TU wrapper for `URIError` — the §19.2.6 malformed-URI
/// raise from the torajs-str `uri.rs` Encode / Decode kernels.
///
/// # Safety
///
/// `msg` must be a valid pointer to a NUL-terminated C string. The
/// caller retains ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_uri_error(msg: *const c_char) {
    // SAFETY: caller invariant — propagated.
    unsafe { throw_native(SLOT_URI_ERROR as i64, msg) };
}

/// §9.2.2 [[Construct]] step 15 — a derived constructor whose body
/// never called `super()` reaches its implicit `return this` with an
/// uninitialized this-binding: ReferenceError. Emitted by the class
/// desugar at the tail of every super-less derived user ctor; the
/// message spelling matches bun/JSC.
///
/// # Safety
/// `extern "C"` ABI; no arguments, records a pending throw only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_no_super_throw() {
    unsafe {
        throw_native(
            SLOT_REFERENCE_ERROR as i64,
            c"'super()' must be called in derived constructor before accessing |this| or returning non-object.".as_ptr(),
        )
    };
}

// bug-327 C2.5 uncaught-throw exit path moved to `uncaught.rs`
// (rotation-196 file-size sweep).
pub mod uncaught;

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    // no_std crate; the test harness re-enables std automatically
    // since cfg(test) ↔ host build. No extra imports needed.
    use super::*;

    // ---- P2.4-b: throw-slot machinery ----

    /// Lock around all throw-slot tests so they don't race on the
    /// global statics. cargo runs tests in parallel by default;
    /// this serializes the ones that touch THROW_ACTIVE/TAG/VALUE.
    /// We use a regular `Mutex` (not parking_lot) to avoid any
    /// crates.io dep.
    static THROW_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Clear the throw slot at test start so a prior test's leak
    /// doesn't pollute the assertion. Returns the mutex guard so
    /// the lock holds for the duration of the test.
    fn fresh_throw_slot() -> std::sync::MutexGuard<'static, ()> {
        let g = THROW_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        THROW_ACTIVE.store(0, Ordering::Relaxed);
        THROW_TAG.store(0, Ordering::Relaxed);
        THROW_VALUE.store(0, Ordering::Relaxed);
        g
    }

    #[test]
    fn throw_check_initially_zero() {
        let _g = fresh_throw_slot();
        unsafe {
            assert_eq!(__torajs_throw_check(), 0);
        }
    }

    #[test]
    fn throw_set_flips_active_and_stores_tag_value() {
        let _g = fresh_throw_slot();
        unsafe {
            __torajs_throw_set(4 /* Heap */, 0xDEADBEEF);
            assert_eq!(__torajs_throw_check(), 1);
            assert_eq!(__torajs_throw_take_tag(), 4);
            // take_tag is non-clearing — check stays 1 until take.
            assert_eq!(__torajs_throw_check(), 1);
            assert_eq!(__torajs_throw_take(), 0xDEADBEEF);
            // take clears active.
            assert_eq!(__torajs_throw_check(), 0);
            // tag stays after take so a later (defensive) read can
            // still see it. value also stays (only active resets).
            assert_eq!(__torajs_throw_take_tag(), 4);
        }
    }

    #[test]
    fn throw_set_overwrites_prior_throw() {
        let _g = fresh_throw_slot();
        unsafe {
            __torajs_throw_set(2, 100);
            __torajs_throw_set(3, 200);
            assert_eq!(__torajs_throw_take_tag(), 3);
            assert_eq!(__torajs_throw_take(), 200);
        }
    }

    #[test]
    fn throw_take_when_inactive_returns_zero_and_stays_clear() {
        let _g = fresh_throw_slot();
        unsafe {
            // Inactive → take returns the stored value (still 0 from
            // fresh_throw_slot) and active stays 0.
            assert_eq!(__torajs_throw_take(), 0);
            assert_eq!(__torajs_throw_check(), 0);
        }
    }
}
