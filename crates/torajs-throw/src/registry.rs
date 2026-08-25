//! Native-error factory registry — the slots `synthesize_module_init`
//! fills with each injected Error subclass's `__new_<C>` factory, and
//! the lookups the runtime raisers next door read them through.
//!
//! Split verbatim out of [`crate`]'s single file, which had reached
//! the 500-line limit when §20.5.7's AggregateError joined. The line
//! is the one the crate doc already draws: this is the registry, and
//! the throw-slot machinery plus the raisers that consult it stay
//! there.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

unsafe extern "C" {
    /// The own-absence Str sentinel (`torajs-str::undef_sentinel`) —
    /// what an omitted ctor `message` stores. Immortal rodata, so no
    /// stake is taken or released across the factory call.
    fn __torajs_str_undef() -> *mut u8;
}

/// `slot` discriminants matching the C ABI:
/// `0` = Error, `1` = TypeError, `2` = RangeError. Read from
/// userspace JS via the EvalError subclass inheriting from Error
/// (slot 0 fallback); the concrete slots cover the runtime-raised
/// cases.
pub const SLOT_ERROR: usize = 0;
pub const SLOT_TYPE_ERROR: usize = 1;
pub const SLOT_RANGE_ERROR: usize = 2;
/// RFC 20260718-error-message-own-prop 刀 3 — the derived-ctor
/// no-super ReferenceError (§9.2.2 [[Construct]] this-TDZ).
pub const SLOT_REFERENCE_ERROR: usize = 3;
/// RFC 20260720-ctor-static-reflection 刀 5b — the §7.1.14
/// StringToBigInt parse-failure SyntaxError (`BigInt("abc")`).
pub const SLOT_SYNTAX_ERROR: usize = 4;
/// §27.2.4.2 — the rejection an all-rejected `Promise.any` answers.
/// The odd one out: its factory carries a DATA param ahead of the
/// shared message, so it is read through [`lookup_aggregate_factory`]
/// rather than [`lookup_factory`]. Registered like the rest.
pub const SLOT_AGGREGATE_ERROR: usize = 5;
/// §19.2.6 — the malformed-URI raise from the `decodeURI` /
/// `decodeURIComponent` / `encodeURI` / `encodeURIComponent`
/// kernels (torajs-str `uri.rs`).
pub const SLOT_URI_ERROR: usize = 6;
pub(crate) const SLOT_COUNT: usize = 7;

/// Class name per slot — the bare-string fallback bakes
/// `"<Name>: "` into the thrown Str so the uncaught reporter (which
/// prints a Str payload verbatim) shows the same first line the
/// instance path renders (RFC 20260825-injection-reachability 刀 A).
/// Index-lockstep with the `SLOT_*` constants above.
pub(crate) const SLOT_NAMES: [&[u8]; SLOT_COUNT] = [
    b"Error",
    b"TypeError",
    b"RangeError",
    b"ReferenceError",
    b"SyntaxError",
    b"AggregateError",
    b"URIError",
];

/// `undefined` in AnyValue NaN-box form (`torajs_anyvalue::nanbox::
/// VALUE_UNDEFINED` = `TAG_BIT_TYPE_OTHER | TAG_BIT_UNDEFINED`).
/// Mirrored rather than imported — torajs-throw is Layer-1 and keeps
/// no upstream crate deps, the same convention as the crate's
/// `ANY_TAG_HEAP`.
///
/// This is what the runtime passes for the ctors' §20.5.8.1 `options`
/// param: a native throw supplies no options bag, and `undefined` is
/// exactly what the TS-level default would have bound.
pub(crate) const ANY_VALUE_UNDEFINED: i64 = 0x0A;

/// Factory fn-ptr type: takes a `*mut Str` (borrowed — the codegen'd
/// TS-level `__new_<C>` fn's ctor field store retains its own
/// reference) plus the §20.5.8.1 `options` param in NaN-box form, and
/// returns a fresh Error-subclass instance with `.message` filled in.
/// The caller keeps its own stake on the Str and must release it
/// after the call.
///
/// **This signature is pinned to the synthesized ctor's parameter
/// list** (`ast::inject_builtin_classes`): codegen emits `__new_<C>`
/// straight from it, so adding or removing a ctor param here without
/// changing that list — or the reverse — leaves the runtime calling
/// with the wrong arity. The second register then holds whatever was
/// left in it, and *every* runtime-thrown TypeError / RangeError is
/// built from garbage while the compiler stays silent. Measured: 99
/// conformance fixtures red, nearly all reading `threw: true` →
/// `threw: false`.
pub type NativeErrorFactory =
    unsafe extern "C" fn(message_str: *mut c_void, options: i64) -> *mut c_void;

/// §20.5.7's factory shape — `__new_AggregateError(errors, message,
/// options)`. The first param is the class's own `any` field, so it
/// arrives as NaN-box bits rather than a pointer; `options` is the
/// same §20.5.8.1 tail every Error ctor carries. Pinned to the
/// synthesized ctor's parameter list exactly as
/// [`NativeErrorFactory`] is.
pub type AggregateErrorFactory =
    unsafe extern "C" fn(errors: i64, message_str: *mut c_void, options: i64) -> *mut c_void;

/// 3-slot registry. `AtomicPtr<()>` rather than `*mut c_void`
/// because raw pointers aren't `Sync`. Each slot is a fn-ptr
/// (typed as `Option<NativeErrorFactory>` after `load`); 4 bytes
/// of padding on 32-bit systems, but Rust pointer width matches
/// host so no layout issue.
static REGISTRY: [AtomicPtr<()>; SLOT_COUNT] = [
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
    AtomicPtr::new(ptr::null_mut()),
];

/// Register a factory for the given slot. Called once at program
/// startup by the codegen'd `synthesize_module_init` for each
/// builtin Error-family class (`Error` / `TypeError` / `RangeError`)
/// emitted by `inject_builtin_classes`.
///
/// `fnptr` is a raw fn-ptr to the codegen'd
/// `__new_<C>(message, options)` factory; out-of-range slots are
/// silently ignored (defensive — codegen always emits valid slots).
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
pub(crate) fn lookup_factory(slot: usize) -> Option<NativeErrorFactory> {
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

/// [`lookup_factory`] for the one slot whose factory takes a data
/// param. Same store, different type — the registry is a bag of
/// fn-ptrs and the slot decides how to read one.
#[inline]
fn lookup_aggregate_factory() -> Option<AggregateErrorFactory> {
    let raw = REGISTRY[SLOT_AGGREGATE_ERROR].load(Ordering::Relaxed);
    if raw.is_null() {
        None
    } else {
        // SAFETY: only `__torajs_register_native_error` writes this
        // slot, and codegen only ever hands it `__new_AggregateError`
        // — the one factory with this shape.
        Some(unsafe { core::mem::transmute::<*mut (), AggregateErrorFactory>(raw) })
    }
}

/// Build an `AggregateError` carrying `errors` (§20.5.7), or NULL
/// when the program has no AggregateError class to build one from.
///
/// `errors` is NaN-box bits for the class's own `any` field; the
/// caller keeps the stake the field store retains. The message is the
/// own-absence Str sentinel, which is what `new AggregateError(xs)`
/// with no second argument stores — §27.2.4.2's error carries no
/// message of its own, so `.message` reads the prototype's `""`.
///
/// A NULL answer is honest rather than defensive: a program that
/// shadows `AggregateError` with its own class, or one compiled
/// before the injector implied it, has no factory to call, and the
/// caller says so in its own terms instead of inventing an object.
///
/// # Safety
///
/// `errors` must be a valid NaN-box the caller owns a stake on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_make_aggregate_error(errors: i64) -> *mut c_void {
    let Some(factory) = lookup_aggregate_factory() else {
        return ptr::null_mut();
    };
    // SAFETY: the factory is the codegen'd `__new_AggregateError`;
    // the sentinel address is immortal read-only rodata.
    unsafe {
        factory(
            errors,
            __torajs_str_undef() as *mut c_void,
            ANY_VALUE_UNDEFINED,
        )
    }
}

#[cfg(test)]
mod tests {
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
        unsafe extern "C" fn sentinel_factory(_msg: *mut c_void, _opts: i64) -> *mut c_void {
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
