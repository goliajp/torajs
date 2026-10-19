//! The async frame against the real universal drop, promise kernels
//! and cycle collector: the dispatcher's AsyncFrame arm, a ring that
//! runs through a parked frame, and the frame layout the three crates
//! read at fixed offsets.

mod common;

// linked for the symbols the promise kernels reach at run time
extern crate torajs_cycle as _;
extern crate torajs_microtask as _;
extern crate torajs_value_drop as _;

use core::ffi::c_void;
use core::mem::offset_of;
use std::sync::{Mutex, MutexGuard};

use common::{drops, malloc_frame_sized, new_frame};
use torajs_cycle::collect::__torajs_cycle_collect;
use torajs_promise::frame::{
    __torajs_frame_await_promise, AWAITING_PARKED, AsyncFrame, FrameDesc, TAG_ASYNC_FRAME,
};
use torajs_rc::{COLOR_MASK, FLAG_BUFFERED, HeapHeader, Tag, rc_inc_cell};
use torajs_value_drop::__torajs_value_drop_heap;

/// The allocator row, the dropguard floor and the collector's buffer
/// are process-wide: one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// `(refcount, flags)`
fn header(p: *mut c_void) -> (u32, u16) {
    let h = p as *const HeapHeader;
    unsafe { ((*h).refcount, (*h).flags) }
}

#[test]
fn the_dispatcher_offers_a_shared_frame_and_drops_it_on_its_last_reference() {
    let _s = serial();
    let before = drops();
    let f = new_frame() as *mut c_void;
    unsafe {
        rc_inc_cell(f);
        __torajs_value_drop_heap(f);
        assert_eq!(drops(), before, "one reference is left");
        assert_eq!(header(f).0, 1);
        assert_ne!(header(f).1 & FLAG_BUFFERED, 0, "a candidate root");

        __torajs_value_drop_heap(f);
        assert_eq!(drops(), before + 1);
        // buffered: the drop leaves the block, black, to the collector
        assert_eq!(header(f).0, 0);
        assert_eq!(header(f).1 & COLOR_MASK, 0);
        assert!((*(f as *const AsyncFrame)).result.is_null());

        __torajs_cycle_collect();
        assert_eq!(torajs_cycle::buffer::len(), 0);
        assert_eq!(
            malloc_frame_sized(),
            f,
            "the collector freed the parked frame"
        );
    }
}

/// `let r = f(); ... await r` inside `f`: the frame parks in its own
/// result's list, which holds it, while it holds the result. Nothing
/// else refers to either; one collection frees both.
#[test]
fn a_frame_awaiting_its_own_result_is_collected_with_it() {
    let _s = serial();
    let before = drops();
    let f = new_frame();
    unsafe {
        let r = (*f).result;
        rc_inc_cell(r);
        __torajs_frame_await_promise(f as *mut c_void, r);
        assert_eq!((*f).awaiting as usize, r as usize | AWAITING_PARKED);
        assert_eq!((header(r).0, header(f as *mut c_void).0), (1, 1));
        assert_ne!(
            header(r).1 & FLAG_BUFFERED,
            0,
            "the result is a candidate root"
        );

        __torajs_cycle_collect();
        assert_eq!(drops(), before + 1, "the frame's drop ran once");
        assert_eq!(torajs_cycle::buffer::len(), 0);
        let mut freed = [malloc_frame_sized(), malloc_frame_sized()];
        freed.sort();
        let mut both = [f as *mut c_void, r];
        both.sort();
        assert_eq!(freed, both, "both cells went back to the allocator");
    }
}

/// torajs-cycle and torajs-value-drop read the frame and its desc at
/// fixed offsets; here all three crates are linked together.
#[test]
fn the_frame_layout_is_read_at_the_offsets_torajs_promise_lays_out() {
    use torajs_cycle::frame as cyc;
    assert_eq!(TAG_ASYNC_FRAME, Tag::AsyncFrame as u16);
    assert_eq!(cyc::TAG_ASYNC_FRAME, TAG_ASYNC_FRAME);
    assert_eq!(cyc::FRAME_DESC_OFF, offset_of!(AsyncFrame, desc));
    assert_eq!(cyc::FRAME_DIRECT_OFF, offset_of!(AsyncFrame, direct));
    assert_eq!(cyc::FRAME_COLLECTED, torajs_promise::frame::FRAME_COLLECTED);
    assert_eq!(cyc::FRAME_NEXT_OFF, offset_of!(AsyncFrame, next));
    assert_eq!(cyc::FRAME_AWAITING_OFF, offset_of!(AsyncFrame, awaiting));
    assert_eq!(cyc::DESC_DROP_OFF, offset_of!(FrameDesc, drop));
    assert_eq!(cyc::DESC_TRACE_OFF, offset_of!(FrameDesc, trace));
}
