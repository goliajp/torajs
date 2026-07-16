//! Closure-face dispatch primitives extracted from
//! [`crate::method_call`] to keep that god-file under the 500-line
//! project cap.
//!
//! The shared shape is the **uniform boxed-adapter ABI**
//! `(env: *mut c_void, argv: *const u64, argc: i64) -> AnyValue`:
//! every closure body a caller might reach through an `any`-typed
//! reference is compiled with a boxed dual entry at
//! `env + CLOSURE_BOXED_ENTRY_OFF` that follows this signature, so
//! dispatch is a single indirect call regardless of the closure's
//! declared arity or param types. `MAX_BOXED_ARGS` fixes the buffer
//! size the adapter reads (padded with `VALUE_UNDEFINED`); wider
//! calls take a heap-sized argv per RFC 20260708 to avoid the
//! silent-truncate the fixed buffer would otherwise imply.
//!
//! Two extern entry points expose this to the SSA lower:
//! - [`__torajs_any_call`] — `f(args…)` where the callee itself is an
//!   `any`. A non-closure receiver / missing adapter is a catchable
//!   TypeError via [`crate::method_call::not_callable`].
//! - [`__torajs_closure_call_variadic`] — raw-pointer twin used when
//!   the callee is Closure-typed but the call site is a variadic
//!   `(...args: E[]) => R` annotation (RFC 20260708-variadic).
//!
//! Callers outside this module import through
//! [`crate::method_call`] via the `pub(crate) use` re-exports there;
//! no consumer needs to know about this sibling by name.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::method_call::not_callable;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

/// Closure-cell boxed dual-entry offset — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_BOXED_ENTRY_OFF`.
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// The boxed adapters read up to 8 param slots unconditionally —
/// mirror of torajs-core `ssa_lower_boxed_entry::MAX_BOXED_PARAMS`.
pub(crate) const MAX_BOXED_ARGS: usize = 8;

/// Resolve a callback-shaped AnyValue to its `(closure cell, boxed
/// dual entry)` pair — `None` for anything that can't dispatch
/// through the uniform ABI (non-cell, non-closure, entry 0).
pub(crate) unsafe fn closure_boxed_entry(av: AnyValue) -> Option<(*mut c_void, u64)> {
    if !is_cell(av) {
        return None;
    }
    unsafe { closure_cell_entry(as_void_ptr(av)) }
}

/// Raw-pointer variant of [`closure_boxed_entry`] for slots that
/// hold the env cell directly (a `Closure`-typed struct field).
pub(crate) unsafe fn closure_cell_entry(ptr: *mut c_void) -> Option<(*mut c_void, u64)> {
    unsafe {
        if ptr.is_null() || (ptr.cast::<u8>().add(4) as *const u16).read() != Tag::Closure as u16 {
            return None;
        }
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == 0 {
            return None;
        }
        Some((ptr, entry))
    }
}

/// Invoke a boxed dual entry through the uniform
/// `(env, argv, argc) -> AnyValue` ABI. argv rides in a fixed
/// 8-slot undefined-filled buffer so the adapter reads its param
/// count unconditionally; the return is caller-owned per the
/// boxed-value convention.
pub(crate) unsafe fn invoke_boxed(
    env: *mut c_void,
    entry: u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let n = argc.max(0) as usize;
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            core::mem::transmute(entry as usize);
        // RFC 20260708-closure-argv-face — an argv-face body reads
        // ALL argc slots off the pointer it receives (the
        // `__torajs_arguments` materializer), so a beyond-buf call
        // takes a heap-sized copy instead of silently truncating;
        // the common ≤ MAX_BOXED_ARGS shape stays on the stack.
        if n > MAX_BOXED_ARGS {
            let mut big = vec![VALUE_UNDEFINED; n];
            for (i, slot) in big.iter_mut().enumerate() {
                *slot = *argv.add(i);
            }
            return call(env, big.as_ptr(), argc);
        }
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        for (i, slot) in buf.iter_mut().enumerate().take(n) {
            *slot = *argv.add(i);
        }
        call(env, buf.as_ptr(), argc)
    }
}

/// `f(args…)` where the callee itself is an `any` value (RFC C4+
/// bare any-call). A `Tag::Closure` cell with a non-zero boxed dual
/// entry invokes through the uniform ABI; every other shape —
/// primitives, non-closure cells, closures without an adapter — is
/// a catchable TypeError. argv slots are BORROWED (the lowerer
/// rc-decs the boxes it made after the call).
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_call(
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_boxed_entry(recv) {
            return invoke_boxed(env, entry, argv, argc);
        }
        not_callable()
    }
}

/// `f(args…)` where `f` is a Closure-typed slot called through a
/// variadic fn-type annotation (`(...args: E[]) => R`) — the
/// raw-pointer twin of [`__torajs_any_call`] (RFC 20260708-variadic:
/// one h body serves closures of any declared arity, so the call
/// dispatches through the boxed dual entry instead of a static
/// declared-pair ABI). A missing adapter (>8 params / unboxable
/// face) is the same catchable TypeError the any-call lane answers.
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call; `env` is a (possibly null) closure env cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_call_variadic(
    env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_cell_entry(env) {
            return invoke_boxed(env, entry, argv, argc);
        }
        not_callable()
    }
}
