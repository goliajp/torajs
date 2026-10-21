//! A body's return settling its result: a result waited on by one
//! parked frame or one inline reaction hands the returning frame's
//! reference on it to that waiter's job; any other list goes through
//! the general drain, in registration order.

use super::*;
use crate::layout::{REPR_I64, STATE_FULFILLED, inline_link};

#[test]
fn a_return_hands_its_result_to_a_sole_parked_frame() {
    let _s = serial();
    let mut log = Vec::new();
    let mut f = frame(0, &mut log);
    let mut w = frame(2, &mut log);
    let result = f.f.result;
    unsafe {
        // one reference for the test, one for the await
        rc_inc_cell(result);
        rc_inc_cell(result);
        __torajs_frame_await_promise(raw(&mut w), result);
        assert_eq!(rc(result), 2, "the frame's and the test's");

        body::__torajs_frame_return(raw(&mut f), REPR_I64 as i64, 7);
        assert!(f.dropped, "the return lets go of the frame");
        assert!(f.f.result.is_null(), "its reference went to the waiter");
        let rp = as_promise(result);
        assert_eq!(((*rp).state, (*rp).value), (STATE_FULFILLED, 7));
        assert!((*rp).callbacks.is_null());
        assert!(w.f.next.is_null());
        assert_eq!(awaiting_owned(&w.f), result, "queued: owned");
        assert_eq!(rc(result), 2, "no reference taken, none released");
        assert!(log.is_empty(), "the waiter's resumption is a job");

        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(2, 7)]);
        assert!(w.dropped);
        assert_eq!(rc(result), 1);
        __torajs_promise_drop(result);
    }
}

#[test]
fn a_return_hands_its_result_to_a_sole_inline_reaction() {
    let _s = serial();
    let mut log = Vec::new();
    let mut f = frame(0, &mut log);
    let result = f.f.result;
    unsafe {
        rc_inc_cell(result);
        let d = __torajs_promise_then_simple(result, Some(double), REPR_I64 as i64);
        assert_eq!((*as_promise(result)).callbacks, inline_link(as_promise(d)));
        body::__torajs_frame_return(raw(&mut f), REPR_I64 as i64, 21);
        assert!(f.dropped && f.f.result.is_null());
        assert_eq!(rc(result), 2, "the test's and the queued reaction's");
        assert_eq!(
            (*as_promise(d)).state,
            STATE_PENDING,
            "the reaction is a job"
        );

        __torajs_microtask_run_until_idle();
        let dp = as_promise(d);
        assert_eq!(((*dp).state, (*dp).value), (STATE_FULFILLED, 42));
        assert_eq!(rc(result), 1);
        __torajs_promise_drop(d);
        __torajs_promise_drop(result);
    }
}

#[test]
fn two_waiters_on_a_returned_result_resume_in_registration_order() {
    let _s = serial();
    let mut log = Vec::new();
    let mut f = frame(0, &mut log);
    let mut a = frame(1, &mut log);
    let mut b = frame(2, &mut log);
    let result = f.f.result;
    unsafe {
        rc_inc_cell(result);
        rc_inc_cell(result);
        rc_inc_cell(result);
        __torajs_frame_await_promise(raw(&mut a), result);
        __torajs_frame_await_promise(raw(&mut b), result);
        body::__torajs_frame_return(raw(&mut f), REPR_I64 as i64, 3);
        assert!(f.dropped && f.f.result.is_null());
        assert_eq!(rc(result), 3, "the test's and one per queued frame");
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 3), (2, 3)]);
        assert!(a.dropped && b.dropped);
        assert_eq!(rc(result), 1);
        __torajs_promise_drop(result);
    }
}

#[test]
fn a_sole_waiter_queues_behind_a_job_already_pending() {
    let _s = serial();
    let mut log = Vec::new();
    let mut f = frame(0, &mut log);
    let mut w = frame(2, &mut log);
    let rec = (100i64, &mut log as *mut Vec<(i64, i64)>);
    let result = f.f.result;
    unsafe {
        rc_inc_cell(result);
        rc_inc_cell(result);
        __torajs_frame_await_promise(raw(&mut w), result);
        let first = crate::pool::alloc_pending();
        __torajs_promise_attach_then(first, Some(record_cb), &rec as *const _ as i64);
        __torajs_promise_resolve(first, 0);
        body::__torajs_frame_return(raw(&mut f), REPR_I64 as i64, 9);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(100, -1), (2, 9)]);
        assert!(w.dropped);
        __torajs_promise_drop(first);
        __torajs_promise_drop(result);
    }
}
