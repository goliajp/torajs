//! The calls a compiled async function body makes on its frame: the
//! await of a value that is not a thenable, the two ways the body
//! ends, and the unwinding of a frame dropped while it waits.
//!
//! A frame dropped at a suspension point still owns whatever the body
//! held there. Its drop does not release those values itself: it
//! re-enters the body at that point with a throw pending
//! ([`__torajs_frame_unwind`]), the body's own exception path releases
//! exactly what it owns, and its exit ([`__torajs_frame_throw`]) sees
//! the frame cancelled and leaves the result alone (pending for good,
//! as nothing can settle what the function waited on). That is the
//! cleanup path of a coroutine destroyed while suspended. A drop can
//! run while a throw is already propagating (a local released on the
//! way out held the last reference to what the frame waited on): the
//! unwind keeps that throw aside and puts it back afterwards.
//!
//! A frame the cycle collector freed is not re-entered: the collect
//! has cleared the words it accounted for, and the frame's trace hands
//! the rest of what the suspension point keeps to the universal drop.

use core::ffi::c_void;
use core::ptr;

use super::{AsyncFrame, FRAME_STATE_DONE, awaiting_owned, desc_resume, desc_trace, release};
use crate::layout::{REPR_ANY, THROW_TAG_I64, as_promise};
use crate::reaction::record::TraceVisit;

unsafe extern "C" {
    fn __torajs_microtask_enqueue(fn_: crate::layout::MicrotaskFn, arg: i64);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_set(tag: i64, value: i64);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_take() -> i64;
}

/// `AsyncFrame::direct` while the frame's drop unwinds its body.
pub const FRAME_CANCELLED: i64 = 1;

/// `AsyncFrame::direct` of a frame the cycle collector is freeing
/// (torajs-cycle `frame` writes it before it runs the drop).
pub const FRAME_COLLECTED: i64 = 2;

/// Set in the trace index of a word the current suspension point
/// keeps (a value the body owns there); the other indices are the
/// links (0 and 1) and the parameter words (their word number).
pub const TRACE_STATE_WORD: i64 = 1 << 40;

/// §27.7.5.3 Await on a value that is statically not a thenable: the
/// PromiseResolve it starts with makes a fulfilled promise nobody can
/// observe, so none is made; the resumption is the one job its
/// reaction would have queued, and the body reads the operand it
/// kept. The frame's reference passes to the queue.
///
/// # Safety
/// `frame` is a live frame its caller owns, not parked or queued.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_await_direct(frame: *mut c_void) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        (*f).awaiting = ptr::null_mut();
        __torajs_microtask_enqueue(desc_resume((*f).desc), frame as i64);
    }
}

/// The body returned `value`, whose form `repr` names: settle the
/// result the way a reaction handler's return settles its derived
/// promise (a promise or a thenable is adopted, anything else
/// fulfils), then let go of the frame. The value's reference goes to
/// the result.
///
/// # Safety
/// `frame` is a live frame its caller owns; `value` is owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_return(frame: *mut c_void, repr: i64, value: i64) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        let result = (*f).result;
        if repr as u8 == REPR_ANY && !box_is_cell(value) {
            // a boxed number or other immediate: nothing to adopt
            let pp = as_promise(result);
            (*pp).value_repr = REPR_ANY;
            (*pp).value_is_heap = 1;
            crate::state::__torajs_promise_resolve(result, value);
        } else {
            crate::then_box::settle_handler_return(result, repr as u8, value);
        }
        (*f).state = FRAME_STATE_DONE;
        release(f);
    }
}

/// The NaN-box gate of `then_adopt::returned_promise`: a cell pointer
/// has its top 16 bits and the other-type tag bit clear.
fn box_is_cell(bits: i64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let b = bits as u64;
    b != 0 && b & TOP_16_MASK == 0 && b & TAG_BIT_TYPE_OTHER == 0
}

/// The body raised: reject the result with the pending throw and let
/// go of the frame. When the frame is being cancelled the throw is
/// the one [`__torajs_frame_unwind`] raised: it is discarded, the
/// result is left pending, and the frame stays with the drop that
/// cancelled it.
///
/// # Safety
/// `frame` is a live frame its caller owns, with a throw pending.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_throw(frame: *mut c_void) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        if (*f).direct == FRAME_CANCELLED {
            __torajs_throw_take_tag();
            __torajs_throw_take();
            return;
        }
        crate::then_box::reject_on_pending_throw((*f).result);
        (*f).state = FRAME_STATE_DONE;
        release(f);
    }
}

/// The drop's unwinding of a frame whose last reference went while it
/// waited at a suspension point. A frame the collector is freeing
/// releases what its trace reports for the point and the collect left
/// in place. Any other is cancelled and re-entered at the point, so
/// the body's exception path releases what it holds; a throw already
/// pending when the drop began is kept aside meanwhile.
///
/// # Safety
/// `frame` is a frame at rc 0 with `state` at a suspension point.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_unwind(frame: *mut c_void) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        if (*f).direct == FRAME_COLLECTED {
            desc_trace((*f).desc)(frame, release_state_word, ptr::null_mut());
            return;
        }
        let active = __torajs_throw_check() != 0;
        let pending = if active {
            (__torajs_throw_take_tag(), __torajs_throw_take())
        } else {
            (0, 0)
        };
        cancel(f);
        desc_resume((*f).desc)(frame as i64);
        if active {
            __torajs_throw_set(pending.0, pending.1);
        }
    }
}

/// Give up the promise the frame holds through `awaiting` (a parked
/// back-pointer is only cut), mark it cancelled and raise the throw
/// its landing propagates.
unsafe fn cancel(f: *mut AsyncFrame) {
    unsafe {
        let src = awaiting_owned(f);
        (*f).awaiting = ptr::null_mut();
        (*f).next = ptr::null_mut();
        __torajs_value_drop_heap(src);
        (*f).direct = FRAME_CANCELLED;
        __torajs_throw_set(THROW_TAG_I64, 0);
    }
}

/// The trace visit of a collected frame's unwinding: a word the
/// suspension point keeps that the collect did not clear is released.
/// The word is left as it is: a word reported twice holds two
/// references, and the frame is freed right after.
unsafe extern "C" fn release_state_word(
    idx: i64,
    child: *mut c_void,
    _slot: *mut c_void,
    _ctx: *mut c_void,
) {
    if idx & TRACE_STATE_WORD != 0 {
        unsafe { __torajs_value_drop_heap(child) };
    }
}

/// The links part of every frame's trace: `result` and an owned
/// `awaiting`, each with the word holding it (indices 0 and 1). The
/// function's own trace adds its parameter words and the words its
/// current suspension point keeps.
///
/// # Safety
/// `frame` is a live frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_frame_trace_links(
    frame: *mut c_void,
    visit: TraceVisit,
    ctx: *mut c_void,
) {
    let f = frame as *mut AsyncFrame;
    unsafe {
        let result = &raw mut (*f).result;
        visit(0, *result, result as *mut c_void, ctx);
        if !awaiting_owned(f).is_null() {
            let awaiting = &raw mut (*f).awaiting;
            visit(1, *awaiting, awaiting as *mut c_void, ctx);
        }
    }
}
