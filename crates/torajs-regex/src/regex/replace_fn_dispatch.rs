//! Replace-callback signature typedefs + dispatcher. Extracted from
//! `replace_fn.rs` to honor the 500-LOC HARD RULE.
//!
//! Twenty C-ABI fn-pointer aliases — 10 base sigs (`Cb0..Cb9`) for
//! the basic `(env, m, g1..gN)` callback shape + 10 with-offset
//! variants (`Cb0Off..Cb9Off`) for the spec-full `(env, m, g1..gN,
//! offset_i64, input_str)` shape (ES §22.1.3.18).
//!
//! `invoke_replace_cb` dispatches `(n_caps, has_off_input)` →
//! transmute + call. Out-of-range `n_caps` (negative or > 9) aborts
//! with a clear stderr message; ssa-lower already rejects > 9 at
//! compile time so the runtime guard is defense in depth.

use core::ffi::c_void;

type Cb0 = unsafe extern "C" fn(env: *mut c_void, m: *mut c_void) -> *mut c_void;
type Cb1 = unsafe extern "C" fn(env: *mut c_void, m: *mut c_void, g1: *mut c_void) -> *mut c_void;
type Cb2 = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
type Cb3 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb4 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb5 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb6 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb7 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb8 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;
type Cb9 = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
) -> *mut c_void;

type Cb0Off = unsafe extern "C" fn(*mut c_void, *mut c_void, i64, *mut c_void) -> *mut c_void;
type Cb1Off =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, i64, *mut c_void) -> *mut c_void;
type Cb2Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb3Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb4Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb5Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb6Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb7Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb8Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;
type Cb9Off = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    i64,
    *mut c_void,
) -> *mut c_void;

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn invoke_replace_cb(
    n_caps: i64,
    has_off_input: bool,
    closure_env: *mut c_void,
    fn_ptr: *mut c_void,
    m: *mut c_void,
    caps: &[*mut c_void; 9],
    off: i64,
    input: *mut c_void,
) -> *mut c_void {
    unsafe {
        if has_off_input {
            invoke_off(n_caps, closure_env, fn_ptr, m, caps, off, input)
        } else {
            invoke_basic(n_caps, closure_env, fn_ptr, m, caps)
        }
    }
}

unsafe fn invoke_basic(
    n_caps: i64,
    env: *mut c_void,
    fn_ptr: *mut c_void,
    m: *mut c_void,
    c: &[*mut c_void; 9],
) -> *mut c_void {
    unsafe {
        match n_caps {
            0 => core::mem::transmute::<*mut c_void, Cb0>(fn_ptr)(env, m),
            1 => core::mem::transmute::<*mut c_void, Cb1>(fn_ptr)(env, m, c[0]),
            2 => core::mem::transmute::<*mut c_void, Cb2>(fn_ptr)(env, m, c[0], c[1]),
            3 => core::mem::transmute::<*mut c_void, Cb3>(fn_ptr)(env, m, c[0], c[1], c[2]),
            4 => core::mem::transmute::<*mut c_void, Cb4>(fn_ptr)(env, m, c[0], c[1], c[2], c[3]),
            5 => core::mem::transmute::<*mut c_void, Cb5>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4],
            ),
            6 => core::mem::transmute::<*mut c_void, Cb6>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5],
            ),
            7 => core::mem::transmute::<*mut c_void, Cb7>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6],
            ),
            8 => core::mem::transmute::<*mut c_void, Cb8>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ),
            9 => core::mem::transmute::<*mut c_void, Cb9>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8],
            ),
            _ => cb_arity_panic(n_caps),
        }
    }
}

unsafe fn invoke_off(
    n_caps: i64,
    env: *mut c_void,
    fn_ptr: *mut c_void,
    m: *mut c_void,
    c: &[*mut c_void; 9],
    off: i64,
    input: *mut c_void,
) -> *mut c_void {
    unsafe {
        match n_caps {
            0 => core::mem::transmute::<*mut c_void, Cb0Off>(fn_ptr)(env, m, off, input),
            1 => core::mem::transmute::<*mut c_void, Cb1Off>(fn_ptr)(env, m, c[0], off, input),
            2 => {
                core::mem::transmute::<*mut c_void, Cb2Off>(fn_ptr)(env, m, c[0], c[1], off, input)
            }
            3 => core::mem::transmute::<*mut c_void, Cb3Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], off, input,
            ),
            4 => core::mem::transmute::<*mut c_void, Cb4Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], off, input,
            ),
            5 => core::mem::transmute::<*mut c_void, Cb5Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], off, input,
            ),
            6 => core::mem::transmute::<*mut c_void, Cb6Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], off, input,
            ),
            7 => core::mem::transmute::<*mut c_void, Cb7Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6], off, input,
            ),
            8 => core::mem::transmute::<*mut c_void, Cb8Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], off, input,
            ),
            9 => core::mem::transmute::<*mut c_void, Cb9Off>(fn_ptr)(
                env, m, c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], off, input,
            ),
            _ => cb_arity_panic(n_caps),
        }
    }
}

fn cb_arity_panic(n: i64) -> ! {
    eprintln!("__torajs_str_replace_regex_fn: n_caps={n} out of range [0,9]");
    std::process::abort();
}
