//! Catchable-throw infrastructure for the torajs AOT TypeScript
//! runtime — slot machinery + native-error factory registry.
//!
//! Layer-1 substrate (no upstream deps). Companion to `torajs-rc`
//! + `torajs-anyvalue` in P2 of the architecture rewrite (see
//! `docs/architecture-rewrite.md`). Replaces the C-side native-
//! error registry + `__torajs_throw_range_error` / `__torajs_throw
//! _type_error` wrappers in `crates/torajs-runtime/src/runtime_str.c`.
//!
//! ## What this crate provides
//!
//! 1. **Native-error factory registry** — three slots (Error,
//!    TypeError, RangeError) into which `synthesize_module_init`
//!    registers each Error subclass's `__new_<C>(message)` factory.
//!    When a runtime helper raises a native error (e.g. bigint
//!    div-by-zero), the registry is consulted to build a real
//!    catchable instance (with proper `.message` / `.name` /
//!    `instanceof` / `.stack`) instead of the legacy bare-string
//!    fallback.
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
//! depend on. The registry is a 3-slot `AtomicPtr<()>` array —
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
use std::sync::atomic::{AtomicPtr, Ordering};

// ============================================================
// Native-error factory registry
// ============================================================

/// `slot` discriminants matching the C ABI:
/// `0` = Error, `1` = TypeError, `2` = RangeError. Read from
/// userspace JS via the SyntaxError / ReferenceError / EvalError /
/// URIError subclasses inheriting from Error (slot 0 fallback);
/// the three concrete slots cover the runtime-raised cases.
pub const SLOT_ERROR: usize = 0;
pub const SLOT_TYPE_ERROR: usize = 1;
pub const SLOT_RANGE_ERROR: usize = 2;
const SLOT_COUNT: usize = 3;

/// Factory fn-ptr type: takes a `*mut Str` (caller transfers
/// ownership of one refcount) and returns a fresh Error-subclass
/// instance with `.message` filled in.
pub type NativeErrorFactory = unsafe extern "C" fn(message_str: *mut c_void) -> *mut c_void;

/// 3-slot registry. `AtomicPtr<()>` rather than `*mut c_void`
/// because raw pointers aren't `Sync`. Each slot is a fn-ptr
/// (typed as `Option<NativeErrorFactory>` after `load`); 4 bytes
/// of padding on 32-bit systems, but Rust pointer width matches
/// host so no layout issue.
static REGISTRY: [AtomicPtr<()>; SLOT_COUNT] = [
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
];

/// Register a factory for the given slot. Called once at program
/// startup by the codegen'd `synthesize_module_init` for each
/// builtin Error-family class (`Error` / `TypeError` / `RangeError`)
/// emitted by `inject_builtin_classes`.
///
/// `fnptr` is a raw fn-ptr to the codegen'd `__new_<C>(message)`
/// factory; out-of-range slots are silently ignored (defensive —
/// codegen always emits valid slots).
///
/// # Safety
///
/// `fnptr` must be either null or a valid fn-ptr matching the
/// `NativeErrorFactory` signature. The pointer is stored without
/// type-checking; calling it from `torajs_throw_native` later
/// transmutes it to the typed fn-ptr.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_register_native_error(slot: i64, fnptr: *mut c_void) {
    if slot < 0 || (slot as usize) >= SLOT_COUNT {
        return;
    }
    REGISTRY[slot as usize].store(fnptr.cast(), Ordering::Relaxed);
}

/// Look up a registered factory; returns `None` if the slot is
/// unregistered (graceful fallback to bare-string throw).
#[inline]
fn lookup_factory(slot: usize) -> Option<NativeErrorFactory> {
    let raw = REGISTRY[slot].load(Ordering::Relaxed);
    if raw.is_null() {
        None
    } else {
        // SAFETY: raw was stored by __torajs_register_native_error
        // which is documented to be called only with valid
        // NativeErrorFactory fn-ptrs. The atomic load returns a
        // bit-equal pointer to what was stored, so the transmute
        // round-trips identity.
        Some(unsafe { core::mem::transmute::<*mut (), NativeErrorFactory>(raw) })
    }
}

// ============================================================
// External Str + throw-slot helpers (still in C / LLVM IR for now)
// ============================================================

unsafe extern "C" {
    /// Allocate a Str with `len` bytes of payload capacity; the
    /// returned ptr's `[header:8][len:u64@8][bytes:len@16]` layout
    /// is pre-initialized except for the bytes (caller writes
    /// those at `*+ 16`). Stays in `crates/torajs-runtime/src/
    /// runtime_str.c` until the Layer-2 `torajs-str` rewrite.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    /// Store `(tag, value)` into the thread-local throw slot and
    /// flag it active. Still LLVM-IR-emitted by ssa_inkwell;
    /// P2.4-b ports it to a Rust static + extern "C" pair.
    fn __torajs_throw_set(tag: i64, value: i64);

    /// libc `strlen` — Layer-0 system primitive; no `dep` cost.
    fn strlen(s: *const c_char) -> usize;
}

/// Byte offset of the Str payload within the heap layout
/// `[header:8][len:8][bytes:N]`. Mirror of the C
/// `__TORAJS_STR_HDR_SIZE` and `torajs-anyvalue`'s `STR_HDR_SIZE`.
const STR_HDR_SIZE: usize = 16;

/// `tag` value matching `AnySlotTag::Heap` — refers to a heap-
/// allocated payload (here a Str or an Error subclass instance).
/// Hard-coded to match the C `__TORAJS_ANY_HEAP` constant and
/// `AnySlotTag::Heap as i64` from `torajs-rc`.
const ANY_TAG_HEAP: i64 = 4;

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

    if slot >= 0 && (slot as usize) < SLOT_COUNT {
        if let Some(factory) = lookup_factory(slot as usize) {
            // SAFETY: factory is a valid NativeErrorFactory per the
            // safety contract of __torajs_register_native_error;
            // err is a freshly-allocated Str the factory takes
            // ownership of.
            let inst = unsafe { factory(err as *mut c_void) };
            // SAFETY: __torajs_throw_set is the LLVM-IR-emitted
            // ABI accepting (tag, value); ANY_TAG_HEAP+inst-ptr is
            // a well-formed packed pair.
            unsafe { __torajs_throw_set(ANY_TAG_HEAP, inst as i64) };
            return;
        }
    }
    // Unregistered slot or out-of-range — bare-string fallback.
    // SAFETY: same as above.
    unsafe { __torajs_throw_set(ANY_TAG_HEAP, err as i64) };
}

/// Cross-TU wrapper: `runtime_bigint.c` / `runtime_regex.c` / etc.
/// call this to raise a catchable `RangeError` (div-by-zero,
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
/// [`__torajs_throw_range_error`]; used by `runtime_regex.c` and
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

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    // no_std crate; the test harness re-enables std automatically
    // since cfg(test) ↔ host build. No extra imports needed.
    use super::*;

    #[test]
    fn slot_constants_match_c_abi() {
        assert_eq!(SLOT_ERROR, 0);
        assert_eq!(SLOT_TYPE_ERROR, 1);
        assert_eq!(SLOT_RANGE_ERROR, 2);
    }

    #[test]
    fn registry_starts_empty() {
        // Slot indices that never had register called — Atomic
        // initializers are null_mut. Verifies the static-init path.
        // Use a fresh slot index to avoid interaction with other
        // tests that may register; SLOT_ERROR is rarely registered
        // in tora's current code so it stays null here.
        assert!(REGISTRY[SLOT_ERROR].load(Ordering::Relaxed).is_null());
    }

    #[test]
    fn register_out_of_range_is_no_op() {
        // No panic, no stash; out-of-range slot silently ignored.
        unsafe {
            __torajs_register_native_error(-1, core::ptr::null_mut::<c_void>().wrapping_add(1));
            __torajs_register_native_error(99, core::ptr::null_mut::<c_void>().wrapping_add(1));
        }
        // Lookups on real slots stay null (nothing was clobbered).
        assert!(REGISTRY[SLOT_ERROR].load(Ordering::Relaxed).is_null());
    }

    #[test]
    fn lookup_factory_null_returns_none() {
        // Empty slot → None.
        assert!(lookup_factory(SLOT_ERROR).is_none());
    }

    #[test]
    fn lookup_factory_after_register_returns_some() {
        // Register a sentinel fn-ptr in SLOT_RANGE_ERROR (we use
        // it explicitly below; ok to leave installed).
        unsafe extern "C" fn sentinel_factory(_msg: *mut c_void) -> *mut c_void {
            0xCAFEF00D as *mut c_void
        }
        let fnptr = sentinel_factory as *mut c_void;
        unsafe {
            __torajs_register_native_error(SLOT_RANGE_ERROR as i64, fnptr);
        }
        assert!(lookup_factory(SLOT_RANGE_ERROR).is_some());
        // Cleanup so other tests aren't perturbed.
        unsafe {
            __torajs_register_native_error(SLOT_RANGE_ERROR as i64, core::ptr::null_mut());
        }
        assert!(lookup_factory(SLOT_RANGE_ERROR).is_none());
    }
}
