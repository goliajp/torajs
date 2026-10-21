//! The frame protocol against the real promise kernels and the real
//! microtask queue: park, settle, resume, a source dying under a
//! parked frame, ordering among list members, and the rule that
//! tells a parked `awaiting` from an owned one. Each test also counts
//! the cycle-root offers its releases make.

use std::sync::{Mutex, MutexGuard};

use super::*;
use crate::layout::{REPR_I64, frame_link, inline_link_derived};
use crate::reaction::record::TraceVisit;
use crate::state::{__torajs_promise_attach_then, __torajs_promise_resolve};
use crate::then::__torajs_promise_then_simple;
use torajs_microtask::__torajs_microtask_run_until_idle;

// The release paths these tests drive reach symbols other runtime
// crates define at `tr build` link time. Nothing here has a weak
// target, a subclass instance or a pending throw, so the stand-ins
// are inert or unreachable.
#[unsafe(no_mangle)]
static __torajs_throw_active: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

#[unsafe(no_mangle)]
unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}

#[unsafe(no_mangle)]
unsafe extern "C" fn __torajs_subclass_drop_entry(_p: *mut c_void) {
    unreachable!("no subclass instance in the frame tests");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __torajs_throw_take() -> i64 {
    unreachable!("no pending throw in the frame tests");
}

#[unsafe(no_mangle)]
unsafe extern "C" fn __torajs_throw_take_tag() -> i64 {
    unreachable!("no pending throw in the frame tests");
}

/// The microtask queue, the pool row and the cycle-root offer count
/// are process-wide: one frame test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn offers() -> usize {
    crate::CYCLE_BUFFER_CALLS.load(core::sync::atomic::Ordering::Relaxed)
}

/// A frame plus what the test reads back: `log` collects
/// `(id, settled value)` per landing, `again` makes the first landing
/// await the same source once more.
#[repr(C)]
struct TestFrame {
    f: AsyncFrame,
    id: i64,
    again: bool,
    dropped: bool,
    log: *mut Vec<(i64, i64)>,
}

/// The test frames' desc, its rows relative to itself like a linked one.
fn desc() -> *const FrameDesc {
    static DESC: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DESC.get_or_init(|| {
        let row = Box::leak(Box::new(FrameDesc {
            resume: 0,
            drop: 0,
            trace: 0,
        })) as *mut FrameDesc;
        unsafe { FrameDesc::fill(row, test_resume, test_drop, no_trace) };
        row as usize
    }) as *const FrameDesc
}

/// The collector is not linked here; nothing walks the frames.
unsafe extern "C" fn no_trace(_: *mut c_void, _: TraceVisit, _: *mut c_void) {}

/// A landing: read the source the job holds, log its value, release
/// it, then either await it again or finish.
unsafe extern "C" fn test_resume(arg: i64) {
    let t = arg as *mut TestFrame;
    unsafe {
        let src = awaiting_owned(&(*t).f);
        assert!(!src.is_null(), "a queued frame owns its source");
        (*(*t).log).push(((*t).id, (*as_promise(src)).value));
        if (*t).again {
            (*t).again = false;
            (*t).f.state = 2;
            __torajs_frame_await_promise(t as *mut c_void, src);
            return;
        }
        (*t).f.awaiting = ptr::null_mut();
        __torajs_promise_drop(src);
        (*t).f.state = FRAME_STATE_DONE;
        __torajs_frame_drop(t as *mut c_void);
    }
}

unsafe extern "C" fn test_drop(p: *mut c_void) {
    let t = p as *mut TestFrame;
    unsafe {
        __torajs_frame_release_links(p);
        (*t).dropped = true;
    }
}

fn frame(id: i64, log: &mut Vec<(i64, i64)>) -> Box<TestFrame> {
    Box::new(TestFrame {
        f: AsyncFrame {
            header: HeapHeader {
                refcount: 1,
                type_tag: TAG_ASYNC_FRAME,
                flags: 0,
            },
            desc: desc(),
            next: ptr::null_mut(),
            awaiting: ptr::null_mut(),
            result: unsafe { __torajs_frame_result_alloc(REPR_I64 as i64) },
            state: 1,
            direct: 0,
        },
        id,
        again: false,
        dropped: false,
        log,
    })
}

fn raw(t: &mut TestFrame) -> *mut c_void {
    t as *mut TestFrame as *mut c_void
}

fn rc(p: *mut c_void) -> u32 {
    unsafe { (*(p as *mut HeapHeader)).refcount }
}

/// A pending source the test keeps one reference on, plus the one it
/// hands to the await.
fn pending_source() -> *mut c_void {
    let src = unsafe { crate::pool::alloc_pending() };
    unsafe { rc_inc_cell(src) };
    src
}

unsafe extern "C" fn double(v: i64) -> i64 {
    v * 2
}

#[test]
fn parked_frame_resumes_once_its_source_settles() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut t = frame(1, &mut log);
    let src = pending_source();
    unsafe {
        __torajs_frame_await_promise(raw(&mut t), src);
        assert_eq!((*as_promise(src)).callbacks, frame_link(&mut t.f));
        assert!(awaiting_owned(&t.f).is_null(), "parked: a back-pointer");
        assert_eq!(t.f.awaiting as usize, src as usize | AWAITING_PARKED);
        assert_eq!(rc(src), 1, "the operand's reference went back");
        assert_eq!((*as_promise(src)).has_handler, 1);
        assert_eq!(
            offers() - before,
            1,
            "a source with a parked frame can close a ring"
        );

        __torajs_promise_resolve(src, 42);
        assert!(t.f.next.is_null());
        assert_eq!(awaiting_owned(&t.f), src, "queued: owned");
        assert_eq!(rc(src), 2, "the job's reference");
        assert!(log.is_empty(), "settlement queues, it does not run");

        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 42)]);
        assert!(t.dropped);
        assert!(t.f.result.is_null());
        assert_eq!(rc(src), 1);
        __torajs_promise_drop(src);
    }
    assert_eq!(
        offers() - before,
        2,
        "and again when the landing lets go of it"
    );
}

#[test]
fn settled_source_queues_without_parking() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut t = frame(1, &mut log);
    let src = pending_source();
    unsafe {
        __torajs_promise_resolve(src, 7);
        __torajs_frame_await_promise(raw(&mut t), src);
        assert!((*as_promise(src)).callbacks.is_null());
        assert_eq!(awaiting_owned(&t.f), src);
        assert_eq!(rc(src), 2);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 7)]);
        assert!(t.dropped);
        assert_eq!(rc(src), 1);
        __torajs_promise_drop(src);
    }
    assert_eq!(offers() - before, 0, "nothing ever parked");
}

#[test]
fn a_dying_source_unlinks_and_drops_its_parked_frame() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut t = frame(1, &mut log);
    // the await gets the only reference: releasing it kills the source
    let src = unsafe { crate::pool::alloc_pending() };
    unsafe {
        __torajs_frame_await_promise(raw(&mut t), src);
        assert!(t.dropped, "the list held the frame's only reference");
        assert!(t.f.awaiting.is_null());
        assert!(t.f.next.is_null());
        assert!(t.f.result.is_null());
        __torajs_microtask_run_until_idle();
    }
    assert!(log.is_empty(), "nothing can resume it");
    assert_eq!(offers() - before, 0);
}

/// A heap record's dispatcher: `arg` points at `(id, log)`.
unsafe extern "C" fn record_cb(arg: i64) {
    let (id, log) = unsafe { *(arg as *const (i64, *mut Vec<(i64, i64)>)) };
    unsafe { (*log).push((id, -1)) };
}

#[test]
fn frames_and_records_resume_in_registration_order() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut a = frame(1, &mut log);
    let mut b = frame(2, &mut log);
    let rec = (100i64, &mut log as *mut Vec<(i64, i64)>);
    let src = pending_source();
    unsafe {
        rc_inc_cell(src);
        __torajs_frame_await_promise(raw(&mut a), src);
        __torajs_promise_attach_then(src, Some(record_cb), &rec as *const _ as i64);
        __torajs_frame_await_promise(raw(&mut b), src);
        assert_eq!((*as_promise(src)).callbacks, frame_link(&mut b.f));
        __torajs_promise_resolve(src, 5);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 5), (100, -1), (2, 5)]);
        assert!(a.dropped && b.dropped);
        assert_eq!(rc(src), 1);
        __torajs_promise_drop(src);
    }
    // two parks, two landings
    assert_eq!(offers() - before, 4);
}

#[test]
fn one_frame_awaits_the_same_source_twice() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut t = frame(3, &mut log);
    t.again = true;
    let src = pending_source();
    unsafe {
        __torajs_frame_await_promise(raw(&mut t), src);
        __torajs_promise_resolve(src, 9);
        __torajs_microtask_run_until_idle();
        assert_eq!(
            log,
            vec![(3, 9), (3, 9)],
            "the second await is one more job"
        );
        assert!(t.dropped);
        assert_eq!(rc(src), 1);
        __torajs_promise_drop(src);
    }
    // the second await finds the source settled and keeps the job's
    // reference: one park, one release at the end
    assert_eq!(offers() - before, 2);
}

#[test]
fn only_an_untagged_awaiting_is_released() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut parked = frame(1, &mut log);
    let mut queued = frame(2, &mut log);
    let src = pending_source();
    unsafe {
        parked.f.awaiting = (src as usize | AWAITING_PARKED) as *mut c_void;
        queued.f.awaiting = src;
        __torajs_frame_release_links(raw(&mut parked));
        assert_eq!(rc(src), 2, "a back-pointer is not a reference");
        __torajs_frame_release_links(raw(&mut queued));
        assert_eq!(rc(src), 1);
        assert!(parked.f.awaiting.is_null() && queued.f.awaiting.is_null());
        assert!(parked.f.result.is_null() && queued.f.result.is_null());
        __torajs_promise_drop(src);
    }
    assert_eq!(offers() - before, 0);
}

/// A source dying with every form in its list: each frame is unlinked
/// and dropped, the inline derived cell loses its back-pointer and the
/// list's reference, the record is freed, and nothing runs.
#[test]
fn a_dying_source_releases_frames_inline_cells_and_records_alike() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut a = frame(1, &mut log);
    let mut b = frame(2, &mut log);
    let rec = (100i64, &mut log as *mut Vec<(i64, i64)>);
    let src = unsafe { crate::pool::alloc_pending() };
    unsafe {
        rc_inc_cell(src);
        __torajs_frame_await_promise(raw(&mut a), src);
        __torajs_promise_attach_then(src, Some(record_cb), &rec as *const _ as i64);
        let d = __torajs_promise_then_simple(src, Some(double), REPR_I64 as i64);
        rc_inc_cell(src);
        __torajs_frame_await_promise(raw(&mut b), src);
        assert_eq!(rc(src), 1);
        assert_eq!(rc(d), 2, "the caller's and the list's");

        __torajs_promise_drop(src);
        assert!(a.dropped && b.dropped);
        assert!(a.f.next.is_null() && b.f.next.is_null());
        let dp = as_promise(d);
        assert!((*dp).rx_source.is_null() && (*dp).rx_next.is_null());
        assert_eq!(rc(d), 1, "the list's reference went");
        __torajs_microtask_run_until_idle();
        assert!(log.is_empty());
        __torajs_promise_drop(d);
    }
    // the two parks; the dying source offers nothing
    assert_eq!(offers() - before, 2);
}

/// A derived cell whose list starts with a parked frame is settled by
/// its handler's return in place: the frame is its sole waiter and
/// nothing else is queued, so it resumes in the same job, holding the
/// dispatcher's stake on `d` (`tests::runframe` has the other cases).
#[test]
fn a_frame_heading_a_derived_list_takes_the_dispatcher_stake() {
    let _s = serial();
    let before = offers();
    let mut log = Vec::new();
    let mut t = frame(1, &mut log);
    let src = unsafe { crate::pool::alloc_pending() };
    unsafe {
        let d = __torajs_promise_then_simple(src, Some(double), REPR_I64 as i64);
        // the caller's reference goes to the await
        __torajs_frame_await_promise(raw(&mut t), d);
        assert_eq!((*as_promise(d)).callbacks, frame_link(&mut t.f));
        assert!(inline_link_derived((*as_promise(d)).callbacks).is_null());

        __torajs_promise_resolve(src, 21);
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(1, 42)]);
        assert!(t.dropped);
        __torajs_promise_drop(src);
    }
    // the park, then the job's release of `src`, marked by the park;
    // the landing's release of `d` is its last
    assert_eq!(offers() - before, 2);
}

#[test]
fn the_result_is_stamped_with_the_body_return_form() {
    let _s = serial();
    unsafe {
        for (repr, heap) in [(REPR_I64, 0), (REPR_STR, 1), (REPR_HEAP, 1), (REPR_ANY, 1)] {
            let p = __torajs_frame_result_alloc(repr as i64);
            let pp = as_promise(p);
            assert_eq!(
                ((*pp).state, (*pp).value_repr, (*pp).value_is_heap),
                (STATE_PENDING, repr, heap)
            );
            __torajs_promise_drop(p);
        }
    }
}

/// The body side: a landing after an await of no thenable, which
/// returns `(id + state) * 10` boxed as an `any` number the way a
/// compiled body returning a number does.
unsafe extern "C" fn direct_resume(arg: i64) {
    let t = arg as *mut TestFrame;
    unsafe {
        assert!(
            (*t).f.awaiting.is_null(),
            "an await of no thenable holds nothing"
        );
        (*(*t).log).push(((*t).id, (*t).f.state));
        let boxed = NUMBER_BOX | (((*t).id + (*t).f.state) * 10);
        body::__torajs_frame_return(t as *mut c_void, REPR_ANY as i64, boxed);
    }
}

/// Not a cell to the NaN-box gate: the top 16 bits are set.
const NUMBER_BOX: i64 = 0x7ffe_0000_0000_0000;

fn direct_desc() -> *const FrameDesc {
    static DESC: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *DESC.get_or_init(|| {
        let row = Box::leak(Box::new(FrameDesc {
            resume: 0,
            drop: 0,
            trace: 0,
        })) as *mut FrameDesc;
        unsafe { FrameDesc::fill(row, direct_resume, test_drop, no_trace) };
        row as usize
    }) as *const FrameDesc
}

#[test]
fn a_direct_await_resumes_one_job_later_and_the_return_fulfills_the_result() {
    let _s = serial();
    let mut log = Vec::new();
    let mut t = frame(4, &mut log);
    t.f.desc = direct_desc();
    let result = t.f.result;
    unsafe {
        rc_inc_cell(result);
        body::__torajs_frame_await_direct(raw(&mut t));
        assert!(log.is_empty(), "the resumption is a job");
        __torajs_microtask_run_until_idle();
        assert_eq!(log, vec![(4, 1)]);
        assert!(t.dropped, "the return lets go of the frame");
        assert_eq!(t.f.state, FRAME_STATE_DONE);
        let rp = as_promise(result);
        assert_eq!(
            ((*rp).state, (*rp).value_repr, (*rp).value),
            (crate::layout::STATE_FULFILLED, REPR_ANY, NUMBER_BOX | 50)
        );
        __torajs_promise_drop(result);
    }
}

#[test]
fn the_trace_reports_the_result_and_only_an_owned_awaiting() {
    unsafe extern "C" fn count(_i: i64, child: *mut c_void, _slot: *mut c_void, ctx: *mut c_void) {
        unsafe { (*(ctx as *mut Vec<usize>)).push(child as usize) };
    }
    let _s = serial();
    let mut log = Vec::new();
    let mut t = frame(1, &mut log);
    let src = pending_source();
    let mut seen: Vec<usize> = Vec::new();
    let ctx = &mut seen as *mut Vec<usize> as *mut c_void;
    unsafe {
        t.f.awaiting = (src as usize | AWAITING_PARKED) as *mut c_void;
        body::__torajs_frame_trace_links(raw(&mut t), count, ctx);
        assert_eq!(seen, vec![t.f.result as usize], "parked: a back-pointer");
        seen.clear();
        t.f.awaiting = src;
        body::__torajs_frame_trace_links(raw(&mut t), count, ctx);
        assert_eq!(seen, vec![t.f.result as usize, src as usize]);
        t.f.awaiting = ptr::null_mut();
        __torajs_promise_drop(src);
        __torajs_promise_drop(src);
        __torajs_frame_release_links(raw(&mut t));
    }
}

mod returns;
mod runframe;
