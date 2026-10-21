//! `wait`: the queue run from the module body's frame.

use super::*;
use crate::wait::{
    __torajs_microtask_await_byte, __torajs_microtask_run_until, __torajs_microtask_yield,
};
use std::sync::atomic::AtomicU8;

static SEEN: Mutex<Vec<i64>> = Mutex::new(Vec::new());
static WATCH: AtomicU8 = AtomicU8::new(0);

fn seen() -> Vec<i64> {
    std::mem::take(&mut *SEEN.lock().unwrap())
}

unsafe extern "C" fn see(arg: i64) {
    SEEN.lock().unwrap().push(arg);
}

/// Sees `arg` and queues `see(arg + 1)`.
unsafe extern "C" fn see_and_queue(arg: i64) {
    SEEN.lock().unwrap().push(arg);
    unsafe { __torajs_microtask_enqueue(Some(see), arg + 1) };
}

unsafe extern "C" fn set_watch(_: i64) {
    WATCH.store(1, Ordering::Relaxed);
}

#[test]
fn the_run_stops_at_the_job_that_sets_the_watch() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    WATCH.store(0, Ordering::Relaxed);
    unsafe {
        __torajs_microtask_enqueue(Some(see), 1);
        __torajs_microtask_enqueue(Some(set_watch), 0);
        __torajs_microtask_enqueue(Some(see), 2);
        assert_eq!(__torajs_microtask_run_until(WATCH.as_ptr()), 1);
        assert_eq!(seen(), vec![1]);
        assert_eq!(__torajs_microtask_pending_count(), 1);
        __torajs_microtask_run_until_idle();
    }
    assert_eq!(seen(), vec![2]);
}

#[test]
fn a_dry_run_answers_0_with_the_counts_restarted() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    WATCH.store(0, Ordering::Relaxed);
    unsafe {
        __torajs_microtask_enqueue(Some(see_and_queue), 1);
        assert_eq!(__torajs_microtask_run_until(WATCH.as_ptr()), 0);
    }
    assert_eq!(seen(), vec![1, 2]);
    assert_eq!(MT_HEAD.load(Ordering::Relaxed), 0);
    assert_eq!(MT_TAIL.load(Ordering::Relaxed), 0);
}

#[test]
fn a_yield_runs_only_the_jobs_ahead_of_it() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    unsafe {
        __torajs_microtask_enqueue(Some(see_and_queue), 1);
        __torajs_microtask_enqueue(Some(see), 5);
        __torajs_microtask_yield();
        // `see(2)` went in behind the continuation
        assert_eq!(seen(), vec![1, 5]);
        assert_eq!(__torajs_microtask_pending_count(), 1);
        __torajs_microtask_run_until_idle();
    }
    assert_eq!(seen(), vec![2]);
}

#[test]
fn a_yield_on_an_empty_queue_goes_on_at_once() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    unsafe { __torajs_microtask_yield() };
    assert!(seen().is_empty());
    assert_eq!(MT_HEAD.load(Ordering::Relaxed), 0);
    assert_eq!(MT_TAIL.load(Ordering::Relaxed), 0);
}

static BYTE: AtomicU8 = AtomicU8::new(0);

unsafe extern "C" fn set_byte(_: i64) {
    BYTE.store(2, Ordering::Relaxed);
}

#[test]
fn an_awaited_byte_goes_on_behind_the_jobs_pending_when_it_is_set() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    BYTE.store(0, Ordering::Relaxed);
    unsafe {
        __torajs_microtask_enqueue(Some(see_and_queue), 1);
        __torajs_microtask_enqueue(Some(set_byte), 0);
        __torajs_microtask_enqueue(Some(see), 5);
        assert_eq!(__torajs_microtask_await_byte(BYTE.as_ptr()), 2);
        // `see(5)` and `see(2)` were pending when the byte was set;
        // nothing was queued after them
        assert_eq!(seen(), vec![1, 5, 2]);
        assert_eq!(__torajs_microtask_pending_count(), 0);
        // set already
        __torajs_microtask_enqueue(Some(see), 7);
        assert_eq!(__torajs_microtask_await_byte(BYTE.as_ptr()), 2);
    }
    assert_eq!(seen(), vec![7]);
}

#[test]
fn an_awaited_byte_left_unset_answers_0() {
    let _g = TEST_LOCK.lock().unwrap();
    reset_queue();
    seen();
    BYTE.store(0, Ordering::Relaxed);
    unsafe {
        __torajs_microtask_enqueue(Some(see), 4);
        assert_eq!(__torajs_microtask_await_byte(BYTE.as_ptr()), 0);
    }
    assert_eq!(seen(), vec![4]);
}

#[test]
fn no_host_job_drain_installed_runs_none() {
    let _g = TEST_LOCK.lock().unwrap();
    assert_eq!(unsafe { crate::wait::__torajs_microtask_host_job() }, 0);
}
