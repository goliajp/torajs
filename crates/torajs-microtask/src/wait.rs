//! The queue run from the module body's own frame (torajs-promise
//! `module_await`). The module body is the root of every job's stack:
//! nothing calls it but `main`, and no job can re-enter it. So when it
//! awaits, the jobs that §9.5 runs before its continuation can run
//! right here, in order, under its frame, and the continuation is the
//! body going on once they have: the same jobs in the same order, with
//! the body's values kept where they are instead of in a frame.
//!
//! Each job ends at a boundary as in the drain (§9.10
//! ClearKeptObjects), and the counts restart whenever the ring runs
//! empty, as the drain restarts them.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicU8, Ordering};

use super::{
    __torajs_microtask_enqueue, HOST_JOB_DRAIN, KEPT_CLEAR, MT_CAP, MT_HEAD, MT_QUEUE, MT_TAIL,
    clear_kept_objects,
};

/// Set by [`mark_job`]: the job queued where a continuation goes has
/// been reached.
static MARK: AtomicU8 = AtomicU8::new(0);

unsafe extern "C" fn mark_job(_: i64) {
    MARK.store(1, Ordering::Relaxed);
}

/// Run the job at the head of the ring, then end it at a boundary.
/// Answers false when there is none.
#[inline(always)]
unsafe fn run_one() -> bool {
    let head = MT_HEAD.load(Ordering::Relaxed);
    if head == MT_TAIL.load(Ordering::Relaxed) {
        return false;
    }
    let slot = head & (MT_CAP.load(Ordering::Relaxed) - 1);
    let t = unsafe { *MT_QUEUE.load(Ordering::Relaxed).add(slot) };
    MT_HEAD.store(head + 1, Ordering::Relaxed);
    unsafe { (t.fn_)(t.arg) };
    unsafe { clear_kept_objects() };
    true
}

/// The ring is empty: restart the counts, as the drain does.
#[inline(always)]
fn reset_if_idle() {
    if MT_HEAD.load(Ordering::Relaxed) == MT_TAIL.load(Ordering::Relaxed) {
        MT_HEAD.store(0, Ordering::Relaxed);
        MT_TAIL.store(0, Ordering::Relaxed);
    }
}

/// End the caller's job, then run jobs in order until `*watch` reads
/// non-zero (answers 1), or until none is left (answers 0; the host
/// jobs are the caller's to run, [`__torajs_microtask_host_job`]).
///
/// # Safety
/// `watch` points to a byte that stays readable while the jobs run;
/// the caller is the module body (no job is running).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_run_until(watch: *const u8) -> i64 {
    unsafe { clear_kept_objects() };
    loop {
        // SAFETY: the caller's contract; a job may have written it
        if unsafe { ptr::read_volatile(watch) } != 0 {
            reset_if_idle();
            return 1;
        }
        if !unsafe { run_one() } {
            MT_HEAD.store(0, Ordering::Relaxed);
            MT_TAIL.store(0, Ordering::Relaxed);
            return 0;
        }
    }
}

/// [`__torajs_microtask_run_until`] for a byte (a cell's state) set by
/// the last call of a job, which queues nothing after it: once it is
/// set, the caller's continuation goes in behind every job pending then
/// ([`__torajs_microtask_yield`]). Answers the byte, the continuation
/// being the next job, or 0 when the queue runs dry first.
///
/// # Safety
/// As [`__torajs_microtask_run_until`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_await_byte(watch: *const u8) -> i64 {
    // SAFETY: the caller's contract
    let byte = unsafe { ptr::read_volatile(watch) };
    if byte == 0 {
        return unsafe { await_byte_run(watch) };
    }
    // a set byte stays as it is while the jobs ahead run
    if byte == 1 {
        return unsafe { __torajs_microtask_yield() };
    }
    unsafe { yield_then(byte as i64) }
}

/// [`__torajs_microtask_yield`], answering `code`.
#[cold]
#[inline(never)]
unsafe fn yield_then(code: i64) -> i64 {
    unsafe { __torajs_microtask_yield() };
    code
}

/// [`__torajs_microtask_await_byte`] with the byte not set yet.
#[inline(never)]
unsafe fn await_byte_run(watch: *const u8) -> i64 {
    unsafe { clear_kept_objects() };
    loop {
        if !unsafe { run_one() } {
            MT_HEAD.store(0, Ordering::Relaxed);
            MT_TAIL.store(0, Ordering::Relaxed);
            return 0;
        }
        // SAFETY: the caller's contract; a job may have written it
        let byte = unsafe { ptr::read_volatile(watch) };
        if byte != 0 {
            if MT_HEAD.load(Ordering::Relaxed) == MT_TAIL.load(Ordering::Relaxed) {
                // the job that set it ended at its boundary
                MT_HEAD.store(0, Ordering::Relaxed);
                MT_TAIL.store(0, Ordering::Relaxed);
            } else {
                unsafe { yield_behind() };
            }
            return byte as i64;
        }
    }
}

/// End the caller's job with its continuation queued behind every job
/// pending now (§27.7.5.3 Await of a settled promise or of a value that
/// is not a thenable: one job, queued at once), and run the jobs ahead
/// of it. With none, the continuation is the next job and the caller
/// goes on at once. Answers 1.
///
/// # Safety
/// The caller is the module body (no job is running).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_yield() -> i64 {
    if MT_HEAD.load(Ordering::Relaxed) != MT_TAIL.load(Ordering::Relaxed) {
        return unsafe { yield_behind() };
    }
    MT_HEAD.store(0, Ordering::Relaxed);
    MT_TAIL.store(0, Ordering::Relaxed);
    if !KEPT_CLEAR.load(Ordering::Relaxed).is_null() {
        return unsafe { clear_kept_then_go_on() };
    }
    1
}

/// The idle [`__torajs_microtask_yield`] with the job-boundary hook
/// installed.
#[inline(never)]
unsafe fn clear_kept_then_go_on() -> i64 {
    unsafe { clear_kept_objects() };
    1
}

/// [`__torajs_microtask_yield`] with jobs pending: queue the mark and
/// run up to it.
#[inline(never)]
unsafe fn yield_behind() -> i64 {
    unsafe { __torajs_microtask_enqueue(Some(mark_job), 0) };
    // the mark is queued, so the run reaches it
    unsafe { __torajs_microtask_run_until(MARK.as_ptr()) };
    MARK.store(0, Ordering::Relaxed);
    1
}

/// Run one host job (§9.13 FinalizationRegistry cleanup), for a module
/// body whose queue ran dry: 1 it ran one, 0 there was none, -1 it
/// threw (the pending throw is the caller's).
///
/// # Safety
/// The caller is the module body (no job is running).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_host_job() -> i32 {
    let hook = HOST_JOB_DRAIN.load(Ordering::Relaxed);
    if hook.is_null() {
        return 0;
    }
    // SAFETY: only `__torajs_host_job_drain_install` stores here.
    let drain = unsafe { core::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i32>(hook) };
    unsafe { drain() }
}
