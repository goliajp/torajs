//! The any-lane half of a call in tail position (§15.10.3
//! PrepareForTailCall): the runtime resolves the callee, the compiled
//! caller jumps.
//!
//! [`__torajs_any_call`] calls the boxed adapter from its own frame,
//! so a strict `return f(x)` through it grows the machine stack by a
//! Rust frame per hop whatever the caller does. A tail-position site
//! instead stores its arguments in [`__torajs_tail_argv`], asks
//! [`__torajs_any_tail_entry`] for the address to enter, and calls it
//! with `(callee, __torajs_tail_argv, argc)` as the last thing it does,
//! which codegen emits as a jump. The argv cannot live in the caller's
//! frame: the frame is gone once the caller jumps.
//!
//! Every argument the site stores is an immediate or an immortal
//! cell, so nothing is left for the caller to release after the call.
//! The callee keeps the borrowed convention every body has. A callee
//! held outside the caller's frame (a parameter, a capture, a global)
//! comes borrowed, as it does to an ordinary call, and a plain closure
//! is entered directly. A callee the frame held comes with the site's
//! reference, which must outlive the call: whoever else holds the
//! closure can let go of it while it runs. That one goes through
//! [`tail_shim_owned`], which makes the ordinary call and releases the
//! reference after it.
//!
//! Single-threaded JS runtime: the argv is a `static mut` read and
//! written by direct index only (the template-object cache's
//! convention); the multi-thread retrofit rides with the rest of the
//! runtime registries.

use crate::method_call::{MAX_BOXED_ARGS, closure_boxed_entry, recv_first_shift};
use crate::method_call_closure_dispatch::__torajs_any_call;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

/// The argv of the call a tail-position site makes, `MAX_BOXED_ARGS`
/// wide because a boxed adapter reads its whole param count. Every
/// adapter loads its slots before it runs anything that can reach
/// another tail-position site.
#[unsafe(no_mangle)]
pub static mut __torajs_tail_argv: [u64; MAX_BOXED_ARGS] = [VALUE_UNDEFINED; MAX_BOXED_ARGS];

/// The entry a tail-position site calls with `(recv, argv, argc)`:
/// [`tail_shim_owned`] when the site holds a reference on `recv`
/// (`owned` non-zero), else `recv`'s boxed adapter when it is a plain
/// closure, else [`tail_shim`]. Pads the argv past `argc` with
/// `undefined`.
///
/// # Safety
/// The site owns one reference on `recv` exactly when `owned` is
/// non-zero; `argc` is at most `MAX_BOXED_ARGS` and the first `argc`
/// argv slots are stored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_tail_entry(recv: AnyValue, argc: i64, owned: i64) -> u64 {
    unsafe {
        let mut i = argc.max(0) as usize;
        while i < MAX_BOXED_ARGS {
            __torajs_tail_argv[i] = VALUE_UNDEFINED;
            i += 1;
        }
        if owned != 0 {
            return tail_shim_owned as *const () as u64;
        }
        plain_closure_entry(recv).unwrap_or(tail_shim as *const () as u64)
    }
}

/// The adapter of a closure [`__torajs_any_call`] would enter with the
/// argv untouched: no Proxy, no class face, no receiver shift.
unsafe fn plain_closure_entry(recv: AnyValue) -> Option<u64> {
    unsafe {
        if crate::proxy::is_proxy(recv) {
            return None;
        }
        let (env, entry) = closure_boxed_entry(recv)?;
        let face = crate::method_value_class::class_accessor_meta(env).is_some()
            || crate::method_value_class::class_method_face_adapter(env).is_some()
            || recv_first_shift(env) != 0;
        (!face).then_some(entry)
    }
}

/// The ordinary call over a borrowed callee the uniform adapter ABI
/// does not reach. The argv is copied out first: the call can run
/// another tail-position site.
unsafe extern "C" fn tail_shim(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        let n = (argc.max(0) as usize).min(MAX_BOXED_ARGS);
        for (i, slot) in buf.iter_mut().enumerate().take(n) {
            *slot = *argv.add(i);
        }
        __torajs_any_call(recv, buf.as_ptr(), argc)
    }
}

/// [`tail_shim`] over a callee the site's reference keeps alive,
/// released once the call is over.
unsafe extern "C" fn tail_shim_owned(recv: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        let out = tail_shim(recv, argv, argc);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(recv);
        out
    }
}
