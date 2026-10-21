//! A derived cell whose sole waiter is a parked frame: its reaction's
//! job fulfils it and resumes the frame in place when that resumption
//! would be the very next job, and queues it otherwise. Each landing
//! records the queue's pop count, which tells the two apart: a frame
//! run in place lands in the pop that ran the reaction.

use std::sync::Mutex;

use super::*;
use torajs_microtask::{
    __torajs_kept_clear_install, __torajs_microtask_head, __torajs_microtask_kept_clear,
    __torajs_microtask_tail,
};

/// `(pop count, push count)` at each landing.
static AT_LANDING: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

unsafe extern "C" fn counted_resume(arg: i64) {
    let at = (
        __torajs_microtask_head.load(core::sync::atomic::Ordering::Relaxed),
        __torajs_microtask_tail.load(core::sync::atomic::Ordering::Relaxed),
    );
    AT_LANDING.lock().unwrap().push(at);
    unsafe { test_resume(arg) }
}

fn counted_desc() -> *const FrameDesc {
    static DESC: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DESC.get_or_init(|| {
        let row = Box::leak(Box::new(FrameDesc {
            resume: 0,
            drop: 0,
            trace: 0,
        })) as *mut FrameDesc;
        unsafe { FrameDesc::fill(row, counted_resume, test_drop, no_trace) };
        row as usize
    }) as *const FrameDesc
}

fn counted_frame(id: i64, log: &mut Vec<(i64, i64)>) -> Box<TestFrame> {
    let mut t = frame(id, log);
    t.f.desc = counted_desc();
    t
}

fn landings() -> Vec<(usize, usize)> {
    std::mem::take(&mut *AT_LANDING.lock().unwrap())
}

/// A pending `src` and `src.then(ret)` with the frame `t` parked on
/// it; answers `src`, which keeps the test's one reference.
fn parked_on_derived(
    t: &mut TestFrame,
    ret: unsafe extern "C" fn(i64) -> i64,
    repr: u8,
) -> *mut c_void {
    unsafe {
        let src = crate::pool::alloc_pending();
        let d = __torajs_promise_then_simple(src, Some(ret), repr as i64);
        __torajs_frame_await_promise(raw(t), d);
        assert_eq!((*as_promise(d)).callbacks, frame_link(&mut t.f));
        src
    }
}

#[test]
fn a_frame_sole_waiter_of_a_derived_cell_runs_in_the_settling_job() {
    let _s = serial();
    landings();
    let mut log = Vec::new();
    let mut t = counted_frame(1, &mut log);
    let src = parked_on_derived(&mut t, double, REPR_I64);
    unsafe {
        __torajs_promise_resolve(src, 21);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 42)]);
        assert_eq!(
            landings(),
            vec![(1, 1)],
            "in the reaction's job, nothing queued"
        );
        assert!(t.dropped);
        __torajs_promise_drop(src);
    }
}

#[test]
fn a_frame_sole_waiter_is_queued_behind_a_pending_job() {
    let _s = serial();
    landings();
    let mut log = Vec::new();
    let mut t = counted_frame(1, &mut log);
    let rec = (100i64, &mut log as *mut Vec<(i64, i64)>);
    let src = parked_on_derived(&mut t, double, REPR_I64);
    unsafe {
        __torajs_promise_resolve(src, 21);
        __torajs_microtask_enqueue(record_cb, &rec as *const _ as i64);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(100, -1), (1, 42)]);
        assert_eq!(
            landings(),
            vec![(3, 3)],
            "a job of its own, after the record's"
        );
        assert!(t.dropped);
        __torajs_promise_drop(src);
    }
}

static CLEARS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

unsafe extern "C" fn count_clear() {
    CLEARS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

#[test]
fn a_frame_sole_waiter_is_queued_when_the_kept_hook_is_installed() {
    let _s = serial();
    landings();
    let mut log = Vec::new();
    let mut t = counted_frame(1, &mut log);
    let src = parked_on_derived(&mut t, double, REPR_I64);
    let before = CLEARS.load(core::sync::atomic::Ordering::Relaxed);
    __torajs_kept_clear_install(count_clear);
    unsafe {
        __torajs_promise_resolve(src, 21);
        __torajs_microtask_run_until_idle();
        __torajs_microtask_kept_clear.store(ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);
        assert_eq!(log, vec![(1, 42)]);
        assert_eq!(landings(), vec![(2, 2)], "a job of its own");
        // one clear before the drain's first pop, one after each job
        assert_eq!(
            CLEARS.load(core::sync::atomic::Ordering::Relaxed) - before,
            3
        );
        __torajs_promise_drop(src);
    }
}

// a handler returning a promise (adopted, two more jobs) needs the rc
// and value externs this test binary stubs out: the conformance
// fixture `promise-then-frame-runnext-004-adopt` covers it

#[test]
fn two_frames_on_a_derived_cell_resume_in_registration_order() {
    let _s = serial();
    landings();
    let mut log = Vec::new();
    let mut a = counted_frame(1, &mut log);
    let mut b = counted_frame(2, &mut log);
    unsafe {
        let src = crate::pool::alloc_pending();
        let d = __torajs_promise_then_simple(src, Some(double), REPR_I64 as i64);
        rc_inc_cell(d);
        __torajs_frame_await_promise(raw(&mut a), d);
        __torajs_frame_await_promise(raw(&mut b), d);
        assert_eq!((*as_promise(d)).callbacks, frame_link(&mut b.f));
        __torajs_promise_resolve(src, 21);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 42), (2, 42)]);
        assert_eq!(landings(), vec![(2, 3), (3, 3)]);
        assert!(a.dropped && b.dropped);
        __torajs_promise_drop(src);
    }
}
