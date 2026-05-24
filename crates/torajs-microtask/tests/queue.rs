//! Black-box tests for the microtask queue. The queue is process-
//! global so we reset between tests by draining + asserting empty.

use std::sync::atomic::{AtomicI64, Ordering};
use torajs_microtask::{
    __torajs_microtask_enqueue, __torajs_microtask_pending_count, __torajs_microtask_run_until_idle,
};

static MARKER: AtomicI64 = AtomicI64::new(0);

unsafe extern "C" fn append_marker(arg: i64) {
    // Bit-OR the arg into the global marker so we can verify
    // FIFO order from the test (each task uses a unique bit).
    MARKER.fetch_or(arg, Ordering::Relaxed);
}

fn drain_clean() {
    unsafe { __torajs_microtask_run_until_idle() };
    assert_eq!(
        unsafe { __torajs_microtask_pending_count() },
        0,
        "queue must be empty after drain"
    );
}

#[test]
fn enqueue_and_drain_runs_each_task() {
    MARKER.store(0, Ordering::Relaxed);
    drain_clean();
    unsafe {
        __torajs_microtask_enqueue(Some(append_marker), 0x1);
        __torajs_microtask_enqueue(Some(append_marker), 0x2);
        __torajs_microtask_enqueue(Some(append_marker), 0x4);
    }
    assert_eq!(unsafe { __torajs_microtask_pending_count() }, 3);
    unsafe { __torajs_microtask_run_until_idle() };
    assert_eq!(MARKER.load(Ordering::Relaxed), 0x7);
    drain_clean();
}

static ORDER: AtomicI64 = AtomicI64::new(0);

unsafe extern "C" fn record_order(arg: i64) {
    // Pack the arg as a base-10 digit into ORDER's running value
    // — verifies tasks ran in FIFO order.
    ORDER.store(ORDER.load(Ordering::Relaxed) * 10 + arg, Ordering::Relaxed);
}

#[test]
fn fifo_order_preserved() {
    ORDER.store(0, Ordering::Relaxed);
    drain_clean();
    unsafe {
        __torajs_microtask_enqueue(Some(record_order), 1);
        __torajs_microtask_enqueue(Some(record_order), 2);
        __torajs_microtask_enqueue(Some(record_order), 3);
        __torajs_microtask_enqueue(Some(record_order), 4);
        __torajs_microtask_run_until_idle();
    }
    assert_eq!(ORDER.load(Ordering::Relaxed), 1234, "FIFO order failed");
    drain_clean();
}

#[test]
fn drain_idempotent_when_empty() {
    drain_clean();
    unsafe { __torajs_microtask_run_until_idle() };
    unsafe { __torajs_microtask_run_until_idle() };
    drain_clean();
}

static REENTRY_COUNT: AtomicI64 = AtomicI64::new(0);

unsafe extern "C" fn reenter_enqueue(arg: i64) {
    REENTRY_COUNT.fetch_add(1, Ordering::Relaxed);
    if arg > 0 {
        // Enqueue a new task with arg-1; spec says it should drain
        // in the same pass.
        unsafe { __torajs_microtask_enqueue(Some(reenter_enqueue), arg - 1) };
    }
}

#[test]
fn reentrant_enqueue_during_drain_extends_pass() {
    REENTRY_COUNT.store(0, Ordering::Relaxed);
    drain_clean();
    unsafe {
        __torajs_microtask_enqueue(Some(reenter_enqueue), 5);
        __torajs_microtask_run_until_idle();
    }
    // Initial task + 5 reentry tasks = 6 calls total.
    assert_eq!(
        REENTRY_COUNT.load(Ordering::Relaxed),
        6,
        "reentrant enqueue must drain in same pass"
    );
    drain_clean();
}

#[test]
fn null_fn_is_silently_skipped() {
    drain_clean();
    // None fn-ptr — spec is "no-op". Verify no panic on drain.
    unsafe {
        __torajs_microtask_enqueue(None, 0);
        __torajs_microtask_run_until_idle();
    }
    drain_clean();
}
