//! The module body's waiter ([`WAITER`]) and its park in a pending
//! promise's list (`super`).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::frame::{AWAITING_PARKED, AsyncFrame, FrameDesc, TAG_ASYNC_FRAME};
use crate::layout::{HeapHeader, Promise, frame_link, link_next};
use crate::pool::__torajs_promise_drop;

unsafe extern "C" {
    fn __torajs_microtask_run_until(watch: *const u8) -> i64;
}

/// Set by the waiter's resumption ([`landing`]).
static READY: AtomicU8 = AtomicU8::new(0);

/// The module body's waiter: a parked frame to every settlement path
/// (`frame` module docs), with no result (NULL) and no slots, so no
/// edge out. It is static and never released: its refcount stays 1
/// for the list or the queue that holds it, and the body's own
/// reference on the awaited promise keeps that promise, and so its
/// list, alive while the waiter is in it. Its header and descriptor
/// are filled on first use (the descriptor's rows are relative to
/// itself), so it takes no bytes of initialized data.
static mut WAITER: AsyncFrame = AsyncFrame {
    header: HeapHeader {
        refcount: 0,
        type_tag: 0,
        flags: 0,
    },
    desc: ptr::null(),
    state: 0,
    direct: 0,
    result: ptr::null_mut(),
    next: ptr::null_mut(),
    awaiting: ptr::null_mut(),
};

static mut WAITER_DESC: FrameDesc = FrameDesc {
    resume: 0,
    drop: 0,
    trace: 0,
};

/// The waiter's resumption: the continuation is the job running now.
/// The settlement handed the waiter a reference on its source as
/// `awaiting`; the body holds its own, so this one goes.
unsafe extern "C" fn landing(frame: i64) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        let src = (*f).awaiting;
        (*f).awaiting = ptr::null_mut();
        READY.store(1, Ordering::Relaxed);
        __torajs_promise_drop(src);
    }
}

/// The waiter is never dropped (see [`WAITER`]).
unsafe extern "C" fn waiter_drop(_frame: *mut c_void) {}

/// The waiter, its descriptor filled.
#[inline(always)]
unsafe fn waiter() -> *mut AsyncFrame {
    let w = &raw mut WAITER;
    unsafe {
        if (*w).desc.is_null() {
            (*w).header = HeapHeader {
                refcount: 1,
                type_tag: TAG_ASYNC_FRAME,
                flags: 0,
            };
            let d = &raw mut WAITER_DESC;
            FrameDesc::fill(
                d,
                landing,
                waiter_drop,
                crate::frame::__torajs_frame_trace_links,
            );
            (*w).desc = d;
        }
    }
    w
}

/// Park the waiter in the pending `sp`'s list and run jobs until its
/// resumption, then answer as [`super::__torajs_module_await_promise`]; or,
/// when the queue runs dry first, take it out again (answers 0).
#[inline(never)]
pub(super) unsafe fn park_and_run(sp: *mut Promise, mask: u32, want: u32) -> i64 {
    unsafe {
        let w = waiter();
        (*w).awaiting = (sp as usize | AWAITING_PARKED) as *mut c_void;
        (*w).next = (*sp).callbacks;
        (*sp).callbacks = frame_link(w);
        if __torajs_microtask_run_until(READY.as_ptr()) == 0 {
            unlink(w, sp);
            return 0;
        }
        READY.store(0, Ordering::Relaxed);
    }
    super::word_code(sp, mask, want)
}

/// Take the parked `w` out of `sp`'s list.
unsafe fn unlink(w: *mut AsyncFrame, sp: *mut Promise) {
    unsafe {
        let link = frame_link(w);
        let mut slot = &raw mut (*sp).callbacks;
        while !(*slot).is_null() {
            if *slot == link {
                *slot = (*w).next;
                break;
            }
            slot = link_next(*slot);
        }
        (*w).next = ptr::null_mut();
        (*w).awaiting = ptr::null_mut();
    }
}
