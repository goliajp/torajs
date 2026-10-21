//! Microtask queue substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (P5, 2026-05-24).
//! Port of the `T-15.c microtask queue` section of
//! `runtime_promise.c` — that section moves here; the rest of the
//! Promise surface stays C-side until P6.1.
//!
//! ## Algorithm
//!
//! Single global FIFO queue. Tasks pushed at the tail via
//! `__torajs_microtask_enqueue`, popped at the head via the drain
//! loop in `__torajs_microtask_run_until_idle`. The backing array is
//! a power-of-two ring (starts at 32 slots) indexed by free-running
//! head / tail counters, and doubles when the ring is full.
//!
//! The `fn` signature is `void (*)(int64_t arg)` — a single i64
//! slot carries either a primitive value or a heap pointer cast
//! through `(int64_t)(intptr_t)`. Codegen for `await` (T-16) and
//! `.then` (T-15.d) both pack `{Promise *, callback closure}` into
//! the arg slot via a small heap struct.
//!
//! Drain semantics: `run_until_idle` returns ONLY when the queue
//! is empty, including tasks enqueued during the drain — matches
//! JS spec's microtask draining (drain to empty before yielding to
//! the event loop / before exit).
//!
//! ## `static mut` 替代 → `AtomicPtr`
//!
//! Same rationale as `torajs-weak::registry` / `torajs-cycle::buffer` /
//! `torajs-arr::pool` — single-threaded runtime today but Rust 2024
//! deprecates raw `static mut`. `AtomicPtr` + `AtomicUsize` + `Relaxed`
//! compiles to identical loads/stores while keeping `&'static` APIs
//! sound + pre-paying the future multi-threaded story.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Public typedef of the microtask function pointer. Matches
/// `runtime_promise.c::__torajs_microtask_fn_t = void (*)(int64_t)`.
/// Kept as a plain `extern "C" fn` so call sites (C-side
/// runtime_promise.c, eventually Rust-side P6.1) get the same ABI.
pub type MicrotaskFn = unsafe extern "C" fn(arg: i64);

/// One queue entry: a function pointer + the i64 arg slot.
#[repr(C)]
#[derive(Copy, Clone)]
struct Microtask {
    fn_: MicrotaskFn,
    arg: i64,
}

/// Ring storage. NULL until first push; `realloc`'d to
/// `MT_CAP × sizeof(Microtask)` on growth.
static MT_QUEUE: AtomicPtr<Microtask> = AtomicPtr::new(ptr::null_mut());

/// Count of tasks popped. A task's slot is its count masked by
/// `MT_CAP - 1`; the pending tasks are the counts `MT_HEAD..MT_TAIL`.
///
/// The two counts and the kept-objects hook are exported under C
/// names: a running promise job reads them in place to tell whether
/// the reaction it is about to queue would be the next job anyway
/// (torajs-promise `reaction::settle_into_chain`).
#[unsafe(no_mangle)]
pub static __torajs_microtask_head: AtomicUsize = AtomicUsize::new(0);
use __torajs_microtask_head as MT_HEAD;

/// Count of tasks pushed.
#[unsafe(no_mangle)]
pub static __torajs_microtask_tail: AtomicUsize = AtomicUsize::new(0);
use __torajs_microtask_tail as MT_TAIL;

/// Ring capacity in entries: 0, or a power of two from 32.
static MT_CAP: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    /// torajs-mmalloc libc-compat realloc — v0.7-A2 step 6b cutover.
    /// First call (cur=NULL) becomes malloc; subsequent calls recover
    /// old size from mmalloc's prepended SHIM_HEADER.
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
}

// v0.7-A2 step 6b — force-link mmalloc.
extern crate torajs_mmalloc as _;

/// The ring is full: double it, then push `(fn_, arg)`. The pending
/// tasks keep their order — the run that wrapped to the front of the
/// old ring moves up behind its end, where the doubled mask finds it,
/// and the counters restart from the old head slot.
#[cold]
#[inline(never)]
fn grow_and_push(fn_: MicrotaskFn, arg: i64) {
    let cap = MT_CAP.load(Ordering::Relaxed);
    let new_cap = if cap == 0 { 32 } else { cap * 2 };
    let elem = core::mem::size_of::<Microtask>();
    let old = MT_QUEUE.load(Ordering::Relaxed);
    let buf = unsafe { realloc(old as *mut c_void, cap * elem, new_cap * elem) } as *mut Microtask;
    let head = MT_HEAD.load(Ordering::Relaxed) & cap.wrapping_sub(1);
    unsafe { ptr::copy_nonoverlapping(buf, buf.add(cap), head) };
    let tail = head + cap;
    unsafe { *buf.add(tail) = Microtask { fn_, arg } };
    MT_QUEUE.store(buf, Ordering::Relaxed);
    MT_CAP.store(new_cap, Ordering::Relaxed);
    MT_HEAD.store(head, Ordering::Relaxed);
    MT_TAIL.store(tail + 1, Ordering::Relaxed);
}

/// Enqueue `(fn, arg)` at the tail of the microtask queue. NULL
/// `fn` is a defensive silent no-op (matches C contract). A full ring
/// doubles first.
///
/// # Safety
/// `fn_` must be a valid C function pointer with the
/// `extern "C" fn(arg: i64)` signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_enqueue(fn_: Option<MicrotaskFn>, arg: i64) {
    let Some(fn_) = fn_ else { return };
    let tail = MT_TAIL.load(Ordering::Relaxed);
    let cap = MT_CAP.load(Ordering::Relaxed);
    if tail - MT_HEAD.load(Ordering::Relaxed) == cap {
        return grow_and_push(fn_, arg);
    }
    let buf = MT_QUEUE.load(Ordering::Relaxed);
    unsafe { *buf.add(tail & (cap - 1)) = Microtask { fn_, arg } };
    MT_TAIL.store(tail + 1, Ordering::Relaxed);
}

/// [`__torajs_microtask_enqueue`] of a job whose argument is a cell,
/// answering the cell: a caller that returns it right after queueing
/// the job reaches this with a tail call and keeps no frame.
///
/// # Safety
/// `fn_` must be a valid C function pointer with the
/// `extern "C" fn(arg: i64)` signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_enqueue_cell(
    fn_: MicrotaskFn,
    cell: *mut c_void,
) -> *mut c_void {
    unsafe { __torajs_microtask_enqueue(Some(fn_), cell as i64) };
    cell
}

/// Drain the microtask queue to empty. Pops one task, runs it,
/// repeats — new tasks enqueued during the callback land at the
/// tail and get processed in this same drain (JS spec microtask
/// semantics: drain to empty before yielding to event loop / exit).
///
/// Auto-called from synthesized `main` at program exit by
/// codegen (T-15.e). A drain that ran a task resets head + tail to 0
/// so the next enqueue starts at slot 0; an empty queue is left as is.
///
/// # Safety
/// Calls user-provided fn pointers; each fn must be a valid
/// `extern "C" fn(i64)` that doesn't reenter `microtask_enqueue`
/// in a way that violates the single-threaded invariant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_run_until_idle() {
    // an empty queue is the drain's whole work at most call sites
    // (every `await` checkpoint); answering it here keeps that case
    // free of the drain loop's frame
    if MT_HEAD.load(Ordering::Relaxed) == MT_TAIL.load(Ordering::Relaxed) {
        // the job that called the drain is over
        unsafe { clear_kept_objects() };
        return;
    }
    unsafe { drain() }
}

/// The caller saw a job pending; clearing the kept objects pops none.
#[inline(never)]
unsafe fn drain() {
    // The job that called the drain is over.
    unsafe { clear_kept_objects() };
    loop {
        let head = MT_HEAD.load(Ordering::Relaxed);
        // a job may grow the ring, so its geometry is read per pop
        let slot = head & (MT_CAP.load(Ordering::Relaxed) - 1);
        let t = unsafe { *MT_QUEUE.load(Ordering::Relaxed).add(slot) };
        MT_HEAD.store(head + 1, Ordering::Relaxed);
        unsafe { (t.fn_)(t.arg) };
        // a job may install the hook (its first WeakRef), so it is read
        // after every job
        unsafe { clear_kept_objects() };
        if MT_HEAD.load(Ordering::Relaxed) == MT_TAIL.load(Ordering::Relaxed) {
            break;
        }
    }
    MT_HEAD.store(0, Ordering::Relaxed);
    MT_TAIL.store(0, Ordering::Relaxed);
}

/// §9.10 ClearKeptObjects — torajs-weak's kept list, installed by its
/// first add, so a program with no `WeakRef` pays one load per job.
/// Exported, as the counts are.
#[unsafe(no_mangle)]
pub static __torajs_microtask_kept_clear: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
use __torajs_microtask_kept_clear as KEPT_CLEAR;

/// Install the kept-objects clear (idempotent).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_kept_clear_install(clear: unsafe extern "C" fn()) {
    KEPT_CLEAR.store(clear as *mut c_void, Ordering::Relaxed);
}

/// # Safety
/// A job boundary: no job is running.
unsafe fn clear_kept_objects() {
    let hook = KEPT_CLEAR.load(Ordering::Relaxed);
    if !hook.is_null() {
        // SAFETY: only `__torajs_kept_clear_install` stores here.
        let clear = unsafe { core::mem::transmute::<*mut c_void, unsafe extern "C" fn()>(hook) };
        unsafe { clear() };
    }
}

/// The drain of the one job kind that lives outside this queue —
/// FinalizationRegistry cleanup (§9.13 HostEnqueueFinalization-
/// RegistryCleanupJob). Installed by the first registry, so a program
/// without one pays a single load at exit and links nothing of it.
/// The hook answers 1 when it ran a job, 0 when there was none, and
/// -1 when a job threw (the pending throw is the caller's).
static HOST_JOB_DRAIN: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Install the host-job drain (idempotent).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_host_job_drain_install(drain: unsafe extern "C" fn() -> i32) {
    HOST_JOB_DRAIN.store(drain as *mut c_void, Ordering::Relaxed);
}

/// The program's exit checkpoint: drain microtasks, then one host job,
/// and again until both are empty — a cleanup callback may queue
/// microtasks or kill further targets. Stops at a job that threw.
///
/// # Safety
/// Same as [`__torajs_microtask_run_until_idle`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_host_jobs_run_until_idle() {
    loop {
        unsafe { __torajs_microtask_run_until_idle() };
        let hook = HOST_JOB_DRAIN.load(Ordering::Relaxed);
        if hook.is_null() {
            return;
        }
        // SAFETY: only `__torajs_host_job_drain_install` stores here.
        let drain =
            unsafe { core::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i32>(hook) };
        if unsafe { drain() } != 1 {
            return;
        }
    }
}

/// `tail - head` — number of pending microtasks. Returns
/// `usize` to match the C `size_t` signature.
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_pending_count() -> usize {
    MT_TAIL.load(Ordering::Relaxed) - MT_HEAD.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests run sequentially because they share the process-global
    // queue. `cargo test --release` (LTO=fat) builds one shared
    // executable per crate, and these tests mutate the static
    // queue state. Use a mutex to serialize.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_queue() {
        MT_HEAD.store(0, Ordering::Relaxed);
        MT_TAIL.store(0, Ordering::Relaxed);
    }

    static mut COUNTER: i64 = 0;

    unsafe extern "C" fn add(arg: i64) {
        unsafe {
            COUNTER += arg;
        }
    }

    #[test]
    fn enqueue_then_drain_sums_args() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe { COUNTER = 0 };
        for v in [1, 2, 3, 4, 5] {
            unsafe { __torajs_microtask_enqueue(Some(add), v) };
        }
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 5);
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
        assert_eq!(unsafe { COUNTER }, 1 + 2 + 3 + 4 + 5);
    }

    static mut REENQUEUE_BUDGET: i32 = 0;

    unsafe extern "C" fn reenqueue(arg: i64) {
        unsafe {
            COUNTER += arg;
            if REENQUEUE_BUDGET > 0 {
                REENQUEUE_BUDGET -= 1;
                __torajs_microtask_enqueue(Some(reenqueue), arg + 1);
            }
        }
    }

    #[test]
    fn drain_processes_reentrant_enqueues() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe {
            COUNTER = 0;
            REENQUEUE_BUDGET = 3;
            __torajs_microtask_enqueue(Some(reenqueue), 10);
            __torajs_microtask_run_until_idle();
        }
        // initial 10, then 11, 12, 13 (budget=3)
        assert_eq!(unsafe { COUNTER }, 10 + 11 + 12 + 13);
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }

    unsafe extern "C" fn noop(_arg: i64) {}

    #[test]
    fn enqueue_above_initial_cap_triggers_grow() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        for i in 0..200 {
            unsafe { __torajs_microtask_enqueue(Some(noop), i) };
        }
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 200);
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }

    static IDLE_IN_JOB: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn note_idle(_arg: i64) {
        let idle = MT_HEAD.load(Ordering::Relaxed) == MT_TAIL.load(Ordering::Relaxed);
        IDLE_IN_JOB.store(idle as usize, Ordering::Relaxed);
    }

    /// The exported counts read equal inside the last pending job and
    /// apart while another job waits behind it.
    #[test]
    fn the_exported_counts_tell_a_job_whether_it_is_the_last() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe { __torajs_microtask_enqueue(Some(note_idle), 0) };
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(IDLE_IN_JOB.load(Ordering::Relaxed), 1);
        unsafe { __torajs_microtask_enqueue(Some(note_idle), 0) };
        unsafe { __torajs_microtask_enqueue(Some(noop), 0) };
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(IDLE_IN_JOB.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn null_fn_is_silent_noop() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe { __torajs_microtask_enqueue(None, 42) };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }
}
