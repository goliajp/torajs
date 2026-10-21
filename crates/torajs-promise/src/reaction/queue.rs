//! The job queue as a settling job sees it: whether a reaction queued
//! now would be the very next job.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

unsafe extern "C" {
    /// torajs-microtask — the queue's pop and push counts and its
    /// job-boundary hook, read in place ([`queue_idle`])
    static __torajs_microtask_head: AtomicUsize;
    static __torajs_microtask_tail: AtomicUsize;
    static __torajs_microtask_kept_clear: AtomicPtr<c_void>;
}

/// Nothing is pending behind the running job and no job-boundary hook
/// is installed: a reaction queued now would be the very next job.
#[inline(always)]
pub(super) unsafe fn queue_idle() -> bool {
    unsafe {
        __torajs_microtask_head.load(Ordering::Relaxed)
            == __torajs_microtask_tail.load(Ordering::Relaxed)
            && __torajs_microtask_kept_clear
                .load(Ordering::Relaxed)
                .is_null()
    }
}
