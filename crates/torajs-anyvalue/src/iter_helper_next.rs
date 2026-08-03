//! GetIteratorDirect's cached next method (§27.1.4.x step 4 /
//! §27.1.6.2) — split beside [`crate::iter_helper`] at the size cap.
//!
//! The spec reads `next` off the underlying ONCE, at helper
//! construction, and every step calls that cached function. Before
//! this module the helpers re-resolved `next` per step through
//! [`crate::iter_any_step::step_derived_iterator`], which re-fires an
//! accessor `next` each time — test262's get-next-method-only-once
//! family minted a fresh generator per Get, so the walk never
//! terminated (8 tr-timeout cases).
//!
//! The cache lane only opens where the re-Get is OBSERVABLE: a
//! receiver whose `next` is a property — an accessor, a
//! closure-valued field, a dynobj entry. A vtable `next` (a generator
//! object, a hand-written iterator class) and the builtin iterator
//! cells (MapIter / ArrIter / a chained IterHelper) keep the legacy
//! driver: their dispatch is not property-observable and their member
//! walk does not answer a callable `next`.
//!
//! Recorded boundary: flatMap / zip INNER iterators and
//! `Iterator.from`'s already-an-Iterator pass-through still step
//! through the legacy driver (the spec caches their next too, but no
//! failing shape reaches them and the inner slot carries no record).

use core::ffi::c_void;

use crate::iter_any_result::iter_result_get;
use crate::iter_helper::{NEXT_OFF, UNDERLYING_OFF};
use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use torajs_rc::Tag;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    /// torajs-structmeta — class-tag → layout, then name → adapter
    /// (the vtable probe [`crate::iter_any`]'s method tier runs).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
}

/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// §7.4.2 GetIteratorDirect at mint / eager-entry time. Answers the
/// OWNED cached next method when the receiver's `next` resolves to a
/// callable property, `VALUE_UNDEFINED` when the legacy per-step
/// driver should keep the receiver (vtable / builtin dispatch, or no
/// callable `next` — that TypeError surfaces at the first step, as
/// today), `Err(())` when the property Get threw (pending throw in
/// flight — the caller forwards it).
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern.
pub(crate) unsafe fn resolve_next_method(recv: AnyValue) -> Result<AnyValue, ()> {
    unsafe {
        if !is_cell(recv) {
            return Ok(VALUE_UNDEFINED);
        }
        let ptr = as_void_ptr(recv) as *mut c_void;
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Obj as u16 {
            // A vtable `next` wins — same order as the step tier; the
            // per-step re-dispatch of a method is not observable.
            let class_tag = ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read();
            let layout = __torajs_struct_layout_lookup(class_tag);
            if !layout.is_null()
                && !__torajs_struct_method_find(layout, c"next".as_ptr() as *const u8, 4).is_null()
            {
                return Ok(VALUE_UNDEFINED);
            }
        } else if tag != Tag::DynObj as u16 {
            // Builtin iterator cells keep their builtin dispatch.
            return Ok(VALUE_UNDEFINED);
        }
        // The real [[Get]] — struct-field fast lane, accessor-aware
        // dispatcher lane (fires a `get next()` exactly once).
        let Some(got) = iter_result_get(recv, ptr, b"next") else {
            return Err(());
        };
        if is_cell(got) && closure_cell_entry(as_void_ptr(got) as *mut c_void).is_some() {
            return Ok(got);
        }
        // Non-callable / absent — the legacy driver's own step-time
        // TypeError keeps the diagnostic.
        __torajs_anyv_rc_dec(got);
        Ok(VALUE_UNDEFINED)
    }
}

/// One §7.4.6 step of `iter` through `next` when one was cached,
/// the legacy driver otherwise. Same contract as
/// [`crate::iter_any_step::step_derived_iterator`]: 1 with `*out`
/// owned on a value, 0 on done / pending throw.
///
/// # Safety
/// `iter` is a live iterator box; `next` is the cached slot value;
/// `out` is writable.
pub(crate) unsafe fn step_via(iter: AnyValue, next: AnyValue, out: *mut AnyValue) -> i64 {
    unsafe {
        if next == VALUE_UNDEFINED {
            return crate::iter_any_step::step_derived_iterator(iter, out, false);
        }
        *out = VALUE_UNDEFINED;
        let Some((env, entry)) = closure_cell_entry(as_void_ptr(next) as *mut c_void) else {
            // Unreachable by construction (resolve cached a callable)
            // — keep the legacy driver rather than UB.
            return crate::iter_any_step::step_derived_iterator(iter, out, false);
        };
        let argv: [u64; 0] = [];
        let step = invoke_with_this(env, entry, iter, argv.as_ptr(), 0);
        if __torajs_throw_check() != 0 {
            return 0;
        }
        if !is_cell(step) {
            __torajs_anyv_rc_dec(step);
            __torajs_throw_type_error(c"iterator result is not an object".as_ptr());
            return 0;
        }
        // §7.4.4 IteratorComplete / §7.4.5 IteratorValue — the shared
        // IteratorResult [[Get]], mirroring the step tier's tail.
        let step_ptr = as_void_ptr(step) as *mut c_void;
        let done = match iter_result_get(step, step_ptr, b"done") {
            None => {
                __torajs_anyv_rc_dec(step);
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
            return 0;
        }
        let value = match iter_result_get(step, step_ptr, b"value") {
            None => {
                __torajs_anyv_rc_dec(step);
                return 0;
            }
            Some(v) => v,
        };
        __torajs_anyv_rc_dec(step);
        *out = value;
        1
    }
}

/// One step of a helper cell's UNDERLYING — reads the cell's cached
/// next slot and rides [`step_via`]. The lazy step loops (map /
/// filter / take / drop / wrap, flatMap's outer) all drive through
/// here.
///
/// # Safety
/// `cell` is a live IterHelper cell; `out` is writable.
pub(crate) unsafe fn helper_underlying_step(cell: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let underlying = (cell.cast::<u8>().add(UNDERLYING_OFF) as *const u64).read();
        let next = (cell.cast::<u8>().add(NEXT_OFF) as *const u64).read();
        step_via(underlying, next, out)
    }
}
