//! The await of a module body (§16.2.1.5.3 Evaluate of a module with
//! top-level await, for a program of one module). The body is the
//! root of every job's stack, so it awaits in place: the jobs §9.5 runs
//! before its continuation run under its frame, in order
//! (torajs-microtask `wait`), and the body goes on once its
//! continuation is the next job. Three ways to know when that is:
//!
//! - a settled promise, or a value that is not a thenable: the
//!   continuation is queued at once (§27.7.5.3 steps 2 and 7), behind
//!   every job pending now (`__torajs_microtask_yield`);
//! - the result of a call to an async function suspending over a frame,
//!   with a form that cannot be adopted (torajs-core
//!   `ssa_lower_fn::suspend::in_place` calls
//!   `__torajs_microtask_await_byte` on its state byte): only the
//!   frame's return or throw settles it, as the last call of its job,
//!   and only this await ever sees it (the call is its operand), so its
//!   settling queues nothing and the continuation goes in right behind
//!   that job. The body runs jobs until the result settles, then as
//!   above;
//! - any other promise ([`__torajs_module_await_promise`]): the body
//!   parks a waiter in its list, a frame with no body (`waiter`), and
//!   the waiter's resumption, queued by the settlement as any parked
//!   frame's is, marks the continuation reached.
//!
//! Each await answers 1 when the continuation is due and the cell holds
//! a fulfillment in the form the body reads, which the body then reads
//! in place; anything else goes through [`__torajs_module_await_slow`]:
//! another form, a rejection (raised), or the queue running dry with
//! the continuation unreached, which runs one host job (§9.13) and
//! waits again, or, with none left, raises the cancel throw: nothing can
//! settle what the body waits on. The cancel throw's path releases what
//! the body holds, and `main` drops the throw and exits 13 with a
//! warning (Node's "unsettled top-level await"). The body keeps its own
//! reference on what it awaits throughout.
//!
//! A throw that escapes the body is reported as uncaught and cleared
//! ([`__torajs_module_body_end`]), as §16.2.1.5.3 rejects only the
//! module's own capability, and `main` goes on draining the queue (§9.5)
//! and exits 1.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::layout::{
    Promise, STATE_FULFILLED, STATE_PENDING, STATE_REJECTED, THROW_TAG_I64, as_promise,
};

mod waiter;

use waiter::park_and_run;

unsafe extern "C" {
    fn __torajs_microtask_await_byte(watch: *const u8) -> i64;
    fn __torajs_microtask_yield() -> i64;
    fn __torajs_microtask_host_job() -> i32;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_set(tag: i64, value: i64);
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_take() -> i64;
    /// torajs-throw: report the pending throw as uncaught and clear it
    fn __torajs_uncaught_report();
    /// torajs-throw: whether [`__torajs_uncaught_report`] ever reported
    fn __torajs_uncaught_seen() -> i32;
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
}

/// The body raised the cancel throw: it never finished.
static UNSETTLED: AtomicU8 = AtomicU8::new(0);

/// Exit code of a program whose module body never finished.
pub const EXIT_UNSETTLED: i32 = 13;

const UNSETTLED_WARNING: &[u8] = b"warning: top-level await never settled\n";

/// Await the promise `src` (the body's reference stays the body's).
/// Answers 1 when, the continuation being the next job, the cell's
/// state word masked by `mask` is `want` (a fulfillment in the form the
/// body reads), 2 when not, 0 when the queue runs dry first.
///
/// # Safety
/// The caller is the module body; `src` is a live promise cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_await_promise(
    src: *mut c_void,
    mask: u32,
    want: u32,
) -> i64 {
    let sp = as_promise(src);
    unsafe {
        (*sp).has_handler = 1;
        if (*sp).state == STATE_PENDING {
            return park_and_run(sp, mask, want);
        }
        // a settled cell stays as it is while the jobs ahead run
        if word_code(sp, mask, want) == 1 {
            return __torajs_microtask_yield();
        }
        yield_then(2)
    }
}

/// The cell's state, `value_is_heap`, `has_handler` and `value_repr`,
/// one byte each from the low end (the layout tests pin them).
fn state_word(sp: *mut Promise) -> *const u32 {
    (sp as usize + core::mem::offset_of!(Promise, state)) as *const u32
}

/// `__torajs_microtask_yield`, answering `code`.
#[cold]
#[inline(never)]
unsafe fn yield_then(code: i64) -> i64 {
    unsafe { __torajs_microtask_yield() };
    code
}

/// 1 when the state word of `sp` masked is `want`, else 2.
#[inline(always)]
fn word_code(sp: *mut Promise, mask: u32, want: u32) -> i64 {
    if unsafe { *state_word(sp) } & mask == want {
        1
    } else {
        2
    }
}

/// `fused` of [`__torajs_module_await_slow`]: the cell is the receiver
/// of a fused `await p.then(f)` (torajs-core
/// `ssa_lower_fn::suspend::then_fused`), whose rejection is raised by
/// the await's continuation, after the jobs ahead of it.
pub const AWAIT_FUSED: i64 = 1;

/// The slow end of the module body's await of the promise `src`, which
/// answered `code`: a queue that ran dry runs one host job, reporting a
/// throw out of it as uncaught, and waits again; with none left it
/// raises the cancel throw (answers 0). Then the value word is read in
/// `want_repr`, converting its form, or the rejection raised.
///
/// # Safety
/// The caller is the module body; `src` is the live cell it awaited.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_await_slow(
    src: *mut c_void,
    code: i64,
    fused: i64,
    want_repr: i64,
) -> i64 {
    let (mask, want) = state_word_wanted(want_repr);
    let again = || unsafe { __torajs_module_await_promise(src, mask, want) };
    unsafe { await_slow(src, code, fused, want_repr, again) }
}

/// [`__torajs_module_await_slow`] for the result of a call to an async
/// function, awaited through `__torajs_microtask_await_byte` on its
/// state.
///
/// # Safety
/// As [`__torajs_module_await_slow`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_await_slow_frame(
    src: *mut c_void,
    code: i64,
    fused: i64,
    want_repr: i64,
) -> i64 {
    let sp = as_promise(src);
    let again = || unsafe { __torajs_microtask_await_byte(&raw const (*sp).state) };
    unsafe { await_slow(src, code, fused, want_repr, again) }
}

#[inline(always)]
unsafe fn await_slow(
    src: *mut c_void,
    mut code: i64,
    fused: i64,
    want_repr: i64,
    again: impl Fn() -> i64,
) -> i64 {
    let sp = as_promise(src);
    unsafe {
        while code == 0 {
            match __torajs_microtask_host_job() {
                0 => {
                    cancel();
                    return 0;
                }
                1 => {}
                _ => __torajs_uncaught_report(),
            }
            code = again();
        }
        if fused & AWAIT_FUSED != 0 && (*sp).state == STATE_REJECTED {
            __torajs_microtask_yield();
        }
        crate::state::__torajs_promise_get_value_as(src, want_repr)
    }
}

/// The state word of a cell fulfilled in the form `want_repr` names,
/// and the bytes of it that tell (torajs-core `ssa_lower_fn::suspend`
/// builds the same two at each await).
fn state_word_wanted(want_repr: i64) -> (u32, u32) {
    if want_repr == 0 {
        (0xFF, STATE_FULFILLED as u32)
    } else {
        (
            0xFF00_00FF,
            ((want_repr as u32) << 24) | STATE_FULFILLED as u32,
        )
    }
}

/// The module body's handler of a fused `await p.then(f)` threw: the
/// await raises it after the jobs ahead of its continuation, which run
/// with the throw kept aside (torajs-core
/// `ssa_lower_fn::suspend::then_fused`).
///
/// # Safety
/// The caller is the module body, with a throw pending.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_yield_rethrow() {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        __torajs_microtask_yield();
        __torajs_throw_set(tag, value);
    }
}

/// The await will never go on: raise the cancel throw, whose path out
/// of the body releases what the body holds.
unsafe fn cancel() {
    UNSETTLED.store(1, Ordering::Relaxed);
    unsafe { __torajs_throw_set(THROW_TAG_I64, 0) };
}

/// Whether the pending throw is the cancel throw: the body's await
/// inside a `try` or a `for…of` takes its own exit on it, running no
/// handler, no `finally` body and no IteratorClose (nothing continues
/// an await that never settles).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_module_cancelled() -> i64 {
    UNSETTLED.load(Ordering::Relaxed) as i64
}

/// The module body returned: a throw it let escape is the cancel throw,
/// dropped, or an uncaught one, reported and cleared.
///
/// # Safety
/// Takes no pointer arguments; called by `main` right after the body.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_body_end() {
    unsafe {
        if __torajs_throw_check() == 0 {
            return;
        }
        if UNSETTLED.load(Ordering::Relaxed) != 0 {
            __torajs_throw_take_tag();
            __torajs_throw_take();
        } else {
            __torajs_uncaught_report();
        }
    }
}

/// The program's exit code, read once the job queue is empty and
/// before any top-level teardown: 13 (with a warning on stderr) when
/// the module body never finished, 1 after an uncaught throw, else the
/// code of the unhandled-rejection sweep, which runs either way.
///
/// # Safety
/// Takes no pointer arguments; call once, at the end of `main`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_module_exit_code() -> i32 {
    let sweep = unsafe { crate::unhandled::__torajs_main_exit_code() };
    if UNSETTLED.load(Ordering::Relaxed) != 0 {
        unsafe { __torajs_syscall_write(2, UNSETTLED_WARNING.as_ptr(), UNSETTLED_WARNING.len()) };
        return EXIT_UNSETTLED;
    }
    if unsafe { __torajs_uncaught_seen() } != 0 {
        return 1;
    }
    sweep
}
