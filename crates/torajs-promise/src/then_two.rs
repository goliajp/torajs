//! Native 2-arg `.then(onOk, onErr)` kernel — ONE attach carrying
//! both handlers (rotation 184).
//!
//! The old lowering desugared the 2-arg form into a
//! `then(onOk) → mid → catch(onErr)` chain (T-19.l "spec
//! equivalent"). Value-wise that holds; microtask-POSITION-wise it
//! does not: a rejected source forwards its rejection to `mid` in
//! tick 1 and only then runs `onErr` from `mid` in tick 2, while
//! the engines run onErr in tick 1 (both handlers attach to the
//! SAME promise per §27.2.5.4 PerformPromiseThen). Probe
//! 2026-07-22: `Promise.reject(2).then(ok, err)` printed two ticks
//! late relative to sibling chains (`A C P B` vs bun `A B C P`).
//!
//! Handler words mirror the 1-arg kernels' `ret_repr` parameter:
//! low byte = return-repr code, bit 8 = PARAM_ANY (dispatch boxes
//! the settled value per the source's repr stamp), bit 9 =
//! THEN2_CLOSURE (the handler pointer is a closure env block whose
//! `env+8` holds the body address; clear = a bare
//! `unsafe extern "C" fn(i64) -> i64`). Mirror:
//! `torajs-core/src/ssa_lower_promise_repr_mark.rs` — must move in
//! lockstep.

use core::ffi::c_void;
use core::ptr;

use crate::layout::{REPR_VOID, STATE_REJECTED, as_promise};
use crate::pool::{__torajs_promise_alloc_pending, __torajs_promise_drop};
use crate::state::__torajs_promise_attach_then;
use crate::then::{ThenCbI64, ThenClosureFn, stamp_result_repr};
use crate::then_box::{
    PARAM_ANY_FLAG, PARAM_REPR_SHIFT, refuse_unstamped, settle_handler_return, settle_param,
};

/// Bit 9 of a then2 handler word — the handler pointer is a closure
/// env block, not a bare fn.
pub const THEN2_CLOSURE_FLAG: i64 = 512;

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
}

const FLAG_PARAM_ANY: u8 = 1;
const FLAG_CLOSURE: u8 = 2;

#[repr(C)]
struct Then2Arg {
    source: *mut c_void,
    on_ok: *mut c_void,
    on_err: *mut c_void,
    result: *mut c_void,
    ok_repr: u8,
    ok_flags: u8,
    ok_param_repr: u8,
    err_repr: u8,
    err_flags: u8,
    err_param_repr: u8,
}

/// Run one handler leg: box the settled value if the handler is
/// any-param, call through the closure env or the bare fn, zero a
/// Void return. The onErr leg RESOLVES the result on normal return
/// (§27.2.5.4 — a rejection handler's return fulfills).
unsafe fn run_leg(handler: *mut c_void, flags: u8, repr: u8, value: i64) -> i64 {
    unsafe {
        let r = if flags & FLAG_CLOSURE != 0 {
            let fn_ptr = *((handler as *mut u8).add(8) as *const *mut c_void);
            let cb: ThenClosureFn = core::mem::transmute(fn_ptr);
            cb(handler, 1, value)
        } else {
            let cb: ThenCbI64 = core::mem::transmute(handler);
            cb(value)
        };
        if repr == REPR_VOID { 0 } else { r }
    }
}

unsafe extern "C" fn then2_dispatch(arg: i64) {
    let a = arg as *mut Then2Arg;
    unsafe {
        let src = as_promise((*a).source);
        let rejected = (*src).state == STATE_REJECTED;
        let (handler, flags, repr, param_repr) = if rejected {
            (
                (*a).on_err,
                (*a).err_flags,
                (*a).err_repr,
                (*a).err_param_repr,
            )
        } else {
            ((*a).on_ok, (*a).ok_flags, (*a).ok_repr, (*a).ok_param_repr)
        };
        let value = settle_param(
            (*src).value_repr,
            flags & FLAG_PARAM_ANY != 0,
            param_repr,
            (*src).value,
        );
        let result = run_leg(handler, flags, repr, value);
        // Either leg may complete abruptly — the throw becomes the
        // result's rejection, including onErr's, whose throw REPLACES
        // the reason it was handed. Otherwise both legs FULFILL on a
        // normal return (onErr's return value is the recovery,
        // §27.2.5.4), adopting it when it is itself a promise.
        settle_handler_return((*a).result, repr, result);
        __torajs_promise_drop((*a).source);
        if (*a).ok_flags & FLAG_CLOSURE != 0 {
            __torajs_value_drop_heap((*a).on_ok);
        }
        if (*a).err_flags & FLAG_CLOSURE != 0 {
            __torajs_value_drop_heap((*a).on_err);
        }
        __torajs_promise_drop((*a).result);
        free(a as *mut c_void);
    }
}

fn split_word(word: i64) -> (u8, u8, u8) {
    let mut flags = 0u8;
    if word & PARAM_ANY_FLAG != 0 {
        flags |= FLAG_PARAM_ANY;
    }
    if word & THEN2_CLOSURE_FLAG != 0 {
        flags |= FLAG_CLOSURE;
    }
    (
        (word & 0xff) as u8,
        flags,
        ((word >> PARAM_REPR_SHIFT) & 0xff) as u8,
    )
}

/// `.then(onOk, onErr)` — both handlers attach to `source` in ONE
/// microtask registration; the dispatcher picks the leg by settled
/// state. Returns the fresh result promise (owned).
///
/// # Safety
///
/// `source` is null or a live promise cell; each handler pointer
/// matches its word's THEN2_CLOSURE flag (closure env block vs bare
/// `fn(i64) -> i64`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_then2(
    source: *mut c_void,
    on_ok: *mut c_void,
    ok_word: i64,
    on_err: *mut c_void,
    err_word: i64,
) -> *mut c_void {
    if source.is_null() || on_ok.is_null() || on_err.is_null() {
        return ptr::null_mut();
    }
    let (ok_repr, ok_flags, ok_param_repr) = split_word(ok_word);
    let (err_repr, err_flags, err_param_repr) = split_word(err_word);
    if (ok_flags | err_flags) & FLAG_PARAM_ANY != 0 && unsafe { refuse_unstamped(source) } {
        return ptr::null_mut();
    }
    let result = unsafe { __torajs_promise_alloc_pending() };
    // Pre-stamp with the fulfilled leg's form — a chained attach
    // lands before this cell settles and its any-lane gate must see
    // a known form; the dispatcher overwrites per the taken leg.
    unsafe { stamp_result_repr(result, ok_repr) };
    let a = unsafe { malloc(core::mem::size_of::<Then2Arg>()) } as *mut Then2Arg;
    unsafe {
        (*a).source = source;
        (*a).on_ok = on_ok;
        (*a).on_err = on_err;
        (*a).result = result;
        (*a).ok_repr = ok_repr;
        (*a).ok_flags = ok_flags;
        (*a).ok_param_repr = ok_param_repr;
        (*a).err_repr = err_repr;
        (*a).err_flags = err_flags;
        (*a).err_param_repr = err_param_repr;
        __torajs_rc_inc(source);
        __torajs_rc_inc(result);
        if ok_flags & FLAG_CLOSURE != 0 {
            __torajs_rc_inc(on_ok);
        }
        if err_flags & FLAG_CLOSURE != 0 {
            __torajs_rc_inc(on_err);
        }
        __torajs_promise_attach_then(source, Some(then2_dispatch), a as i64);
    }
    result
}
