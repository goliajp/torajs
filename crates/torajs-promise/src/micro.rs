//! `queueMicrotask(cb)` global — closure + named-fn variants.
//!
//! Port of `runtime_promise.c` P10.1-A1 section (P6.1, 2026-05-24).
//! ssa_lower picks the right variant based on cb's static type:
//!
//! - `Type::Closure` → `queue_microtask_closure(env)` — env block
//!   carries fn_addr at +8 + captures at +24. Dispatcher loads
//!   fn_addr, calls cb(env), drops env via universal heap-header
//!   dispatch (the env layout's drop_fn at +16 handles per-closure
//!   captures cleanup).
//! - `Type::FnSig` → `queue_microtask_simple(fn_ptr)` — raw fn
//!   pointer, no env. Dispatcher casts back + calls.
//!
//! Both helpers carry the cb/env through the microtask queue's i64
//! `arg` slot — no wrapper struct, just a single-field pack.

use core::ffi::c_void;

use crate::layout::MicrotaskFn;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_microtask_enqueue(fn_: MicrotaskFn, arg: i64);
}

/// Closure dispatcher. Reads fn_addr from env+8, calls cb(env),
/// then drops the env ref so captures + env block release exactly
/// once after the task fires.
unsafe extern "C" fn queue_micro_closure_dispatch(arg: i64) {
    let env = arg as *mut c_void;
    unsafe {
        let fn_ptr = *((env as *mut u8).add(8) as *const *mut c_void);
        let cb: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(fn_ptr);
        cb(env);
        __torajs_value_drop_heap(env);
    }
}

/// `queueMicrotask(closureCb)`. Callers pass the env block; we inc
/// to keep it alive across the microtask delay; the dispatcher
/// pairs that with `value_drop_heap`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_queue_microtask_closure(env: *mut c_void) {
    if env.is_null() {
        return;
    }
    unsafe {
        __torajs_rc_inc(env);
        __torajs_microtask_enqueue(queue_micro_closure_dispatch, env as i64);
    }
}

/// Simple-fn dispatcher — raw fn ptr, no env, no return.
unsafe extern "C" fn queue_micro_simple_dispatch(arg: i64) {
    let cb: unsafe extern "C" fn() = unsafe { core::mem::transmute(arg as *const c_void) };
    unsafe { cb() };
}

/// `queueMicrotask(namedFn)`. Fn pointers aren't heap objects so no
/// rc bookkeeping is needed — the code object lives in the binary's
/// .text segment for the lifetime of the program.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_queue_microtask_simple(fn_ptr: *mut c_void) {
    if fn_ptr.is_null() {
        return;
    }
    unsafe { __torajs_microtask_enqueue(queue_micro_simple_dispatch, fn_ptr as i64) };
}
