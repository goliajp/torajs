//! A discarded chain of async frames, each parked on the result of
//! the one before: the death of the first source releases every frame
//! and result in turn. Each step goes through the universal drop, so
//! the release defers at the stack floor instead of nesting once per
//! frame. Its own test binary: the floor is process-wide and is set
//! from the stack of the first deep release.

mod common;

// linked for the symbols the promise kernels reach at run time
extern crate torajs_cycle as _;
extern crate torajs_microtask as _;
extern crate torajs_value_drop as _;

/// Deeper than an undeferred release fits in the thread's stack.
const FRAMES: usize = 100_000;

/// The main thread's stack size: the floor is placed by it.
const STACK: usize = 8 << 20;

#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
#[test]
fn a_discarded_chain_of_parked_frames_releases_at_bounded_depth() {
    use core::ffi::c_void;
    use torajs_promise::frame::__torajs_frame_await_promise;
    use torajs_promise::{__torajs_promise_alloc_pending, __torajs_promise_drop};
    use torajs_rc::rc_inc_cell;

    let released = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| unsafe {
            let src = __torajs_promise_alloc_pending();
            let mut awaited = src;
            for _ in 0..FRAMES {
                let f = common::new_frame();
                // the await's operand reference
                rc_inc_cell(awaited);
                __torajs_frame_await_promise(f as *mut c_void, awaited);
                awaited = (*f).result;
            }
            let before = common::drops();
            __torajs_promise_drop(src);
            common::drops() - before
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(released, FRAMES);
}
