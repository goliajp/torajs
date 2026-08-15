//! §7.4.6 IteratorNext step tier over an already-derived iterator —
//! split from [`crate::iter_any`] at the size cap. One fn: the
//! vtable-first / generic-fallback step both GetIterator lanes and
//! the `Tag::Obj` receiver lane drive, sync and `for await` alike.

use core::ffi::c_void;

use crate::iter_any::{MethodOutcome, call_obj_method_0};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
}

/// One step of an already-derived iterator, whichever way it was
/// found — §7.4.6 IteratorNext plus §7.4.4/§7.4.5 on the result.
///
/// The tier split is by how `next` DISPATCHES, and `Tag::Obj` is not
/// the answer to that: an anon-stamped object literal wears the same
/// tag as a class instance, but its `next` is a closure-valued FIELD,
/// not a vtable method. So the vtable is tried first and a miss falls
/// through to the generic tier rather than becoming an error — the
/// same entry-probe-then-by-name shape the member cascades use. A
/// generator object and a hand-written iterator class keep the exact
/// path they had.
///
/// `await_mode` (§14.7.5.6 async-iterate): a `next()` that answered a
/// Promise — an async generator's, a user async iterator's — is
/// awaited before §7.4.4 reads the result; a sync result's VALUE is
/// awaited instead, standing in for the §27.1.4.4 Async-from-Sync
/// wrapper. The Promise-or-not probe on the step result IS the
/// protocol split — no lane bookkeeping needed.
///
/// # Safety
/// `iter` is the caller's live iterator box; `out` is writable.
pub(crate) unsafe fn step_derived_iterator(
    iter: AnyValue,
    out: *mut AnyValue,
    await_mode: bool,
) -> i64 {
    unsafe {
        if !is_cell(iter) {
            __torajs_throw_type_error(c"iterator is not an object".as_ptr());
            *out = VALUE_UNDEFINED;
            return 0;
        }
        let iter_ptr = as_void_ptr(iter) as *mut c_void;
        if (iter_ptr.cast::<u8>().add(4) as *const u16).read() != Tag::Obj as u16 {
            return crate::iter_any_get_method::generic_iter_step(iter, out, await_mode);
        }
        // ES §7.4.6 IteratorNext — a step that throws forwards the
        // abrupt completion; there is no result to inspect.
        let mut step = match call_obj_method_0(iter_ptr, b"next") {
            MethodOutcome::Ok(step) => step,
            // Not a vtable method — an object literal keeps `next` as
            // a closure-valued field. Same iterator, other tier; the
            // "no next()" diagnostic belongs to that tier's dispatcher.
            MethodOutcome::Missing => {
                return crate::iter_any_get_method::generic_iter_step(iter, out, await_mode);
            }
            MethodOutcome::Threw => {
                *out = VALUE_UNDEFINED;
                return 0;
            }
        };
        let was_async = await_mode && crate::iter_any_await::await_settle(&mut step);
        if was_async && __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(step);
            *out = VALUE_UNDEFINED;
            return 0;
        }
        // §7.4.4 step 1 — the result must be an OBJECT; a primitive
        // heap cell (a string is a Str cell in tr) is not one, and
        // reading done/value off it spun this driver forever (the
        // step_via twin had the identical hole — rotation 408).
        if !is_cell(step)
            || crate::iter_helper_next::is_primitive_heap_cell(as_void_ptr(step) as *const c_void)
        {
            __torajs_anyv_rc_dec(step);
            __torajs_throw_type_error(c"iterator result is not an object".as_ptr());
            *out = VALUE_UNDEFINED;
            return 0;
        }
        let step_ptr = as_void_ptr(step) as *mut c_void;
        // §7.4.4 IteratorComplete — ToBoolean(Get(step, "done")). The
        // Get may run a getter that throws (forward it); the answer is
        // truthiness, not a Bool-tag check (`{done: 1}` is done).
        let done = match crate::iter_any_result::iter_result_get(step, step_ptr, b"done") {
            None => {
                __torajs_anyv_rc_dec(step);
                *out = VALUE_UNDEFINED;
                return 0;
            }
            Some(v) => {
                let b = crate::nanbox_ffi::__torajs_anyv_to_bool(v);
                __torajs_anyv_rc_dec(v);
                b
            }
        };
        if done {
            __torajs_anyv_rc_dec(step);
            *out = VALUE_UNDEFINED;
            return 0;
        }
        // §7.4.5 IteratorValue — Get(step, "value"), abrupt forwarded
        // (the test262 poisoned-getter shape: the ONLY loop exit).
        let value = match crate::iter_any_result::iter_result_get(step, step_ptr, b"value") {
            None => {
                __torajs_anyv_rc_dec(step);
                *out = VALUE_UNDEFINED;
                return 0;
            }
            Some(v) => v,
        };
        __torajs_anyv_rc_dec(step);
        *out = value;
        if await_mode && !was_async {
            return crate::iter_any_await::settle_out(out);
        }
        1
    }
}
