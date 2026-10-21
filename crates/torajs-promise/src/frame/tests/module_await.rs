//! The module body's await (`module_await`) against the real queue:
//! where its continuation goes among the jobs pending and queued while
//! it waits, and the queue running dry under it.

use std::sync::Mutex;

use super::*;
use crate::module_await::__torajs_module_await_promise;
use torajs_microtask::wait::__torajs_microtask_await_byte;
use torajs_microtask::{__torajs_microtask_enqueue, __torajs_microtask_pending_count};

// reached only through a host job that threw; none runs here
#[unsafe(no_mangle)]
unsafe extern "C" fn __torajs_uncaught_report() {
    unreachable!("no host job in the module await tests");
}

static ORDER: Mutex<Vec<i64>> = Mutex::new(Vec::new());

fn order() -> Vec<i64> {
    std::mem::take(&mut *ORDER.lock().unwrap())
}

unsafe extern "C" fn note(arg: i64) {
    ORDER.lock().unwrap().push(arg);
}

/// Notes 1, queues `note(2)`.
unsafe extern "C" fn note_then_queue(_: i64) {
    ORDER.lock().unwrap().push(1);
    unsafe { __torajs_microtask_enqueue(Some(note), 2) };
}

/// Notes 1 and fulfils the promise `arg` with 5, as its last call.
unsafe extern "C" fn settle_last(arg: i64) {
    ORDER.lock().unwrap().push(1);
    unsafe { __torajs_promise_resolve(arg as *mut c_void, 5) };
}

/// Fulfils the promise `arg`, then queues `note(3)` behind the
/// waiter's resumption.
unsafe extern "C" fn settle_then_queue(arg: i64) {
    ORDER.lock().unwrap().push(1);
    unsafe {
        __torajs_promise_resolve(arg as *mut c_void, 5);
        __torajs_microtask_enqueue(Some(note), 3);
    }
}

#[test]
fn a_settled_promise_goes_on_behind_the_jobs_pending_now() {
    let _s = serial();
    order();
    unsafe {
        let src = crate::pool::alloc_pending();
        __torajs_promise_resolve(src, 9);
        __torajs_microtask_enqueue(Some(note_then_queue), 0);
        assert_eq!(await_promise(src), 1);
        // `note(2)` was queued behind the continuation
        assert_eq!(order(), vec![1]);
        assert_eq!((*as_promise(src)).has_handler, 1);
        __torajs_microtask_run_until_idle();
        assert_eq!(order(), vec![2]);
        __torajs_promise_drop(src);
    }
}

#[test]
fn a_pending_promise_goes_on_at_its_waiters_resumption() {
    let _s = serial();
    order();
    unsafe {
        let src = crate::pool::alloc_pending();
        __torajs_microtask_enqueue(Some(settle_then_queue), src as i64);
        assert_eq!(await_promise(src), 1);
        assert_eq!(order(), vec![1]);
        assert!((*as_promise(src)).callbacks.is_null());
        assert_eq!(rc(src), 1, "the resumption let go of its reference");
        __torajs_microtask_run_until_idle();
        assert_eq!(order(), vec![3]);
        __torajs_promise_drop(src);
    }
}

#[test]
fn a_dry_queue_answers_0_and_takes_the_waiter_out() {
    let _s = serial();
    order();
    let mut log = Vec::new();
    let mut t = frame(7, &mut log);
    unsafe {
        let src = pending_source();
        __torajs_frame_await_promise(raw(&mut t), src);
        assert_eq!(await_promise(src), 0);
        // the waiter is out; the frame parked before it is still in
        assert_eq!((*as_promise(src)).callbacks, frame_link(&mut t.f));
        assert!(t.f.next.is_null());
        assert_eq!(rc(src), 1);
        __torajs_promise_drop(src);
    }
    assert!(t.dropped, "the source died with the frame parked");
}

#[test]
fn a_frame_result_goes_on_right_behind_its_settling_job() {
    let _s = serial();
    order();
    unsafe {
        let src = __torajs_frame_result_alloc(REPR_I64 as i64);
        __torajs_microtask_enqueue(Some(settle_last), src as i64);
        __torajs_microtask_enqueue(Some(note), 4);
        assert_eq!(await_state(src), 1);
        // the job queued before the result settled runs first
        assert_eq!(order(), vec![1, 4]);
        assert_eq!((*as_promise(src)).value, 5);
        __torajs_promise_drop(src);
    }
}

#[test]
fn a_frame_result_settled_with_nothing_queued_goes_on_at_once() {
    let _s = serial();
    order();
    unsafe {
        let src = __torajs_frame_result_alloc(REPR_I64 as i64);
        __torajs_microtask_enqueue(Some(settle_last), src as i64);
        assert_eq!(await_state(src), 1);
        assert_eq!(order(), vec![1]);
        assert_eq!(__torajs_microtask_pending_count(), 0);
        __torajs_promise_drop(src);
    }
}

/// The module body's await of a frame result: its state byte.
unsafe fn await_state(src: *mut c_void) -> i64 {
    unsafe { __torajs_microtask_await_byte(&raw const (*as_promise(src)).state) }
}

/// The module body's await of any promise, as [`await_state`].
unsafe fn await_promise(src: *mut c_void) -> i64 {
    unsafe { __torajs_module_await_promise(src, 0xFF, 1) }
}
