//! `new v()` over a unit-class object (RFC 20260917-class-eval-unit
//! K1): one evaluation of a class declared in a function or loop body.
//!
//! Such a class object is not in the by-cell constructor table — every
//! evaluation mints a new one, and the table holds the one registered
//! template. Its hidden record names the class tag; the template's
//! factory entry is the same code, and it takes the evaluation as its
//! leading argument (the `__unit` parameter every `new C(…)` in the
//! declaring scope passes).

use core::ffi::c_void;

use crate::method_call_closure_dispatch::invoke_boxed;
use crate::nanbox::{AnyValue, as_void_ptr};

unsafe extern "C" {
    /// torajs-meta `classmeta::unit` — the tag a unit-class object
    /// records, or -1.
    fn __torajs_class_unit_tag_of(cell: *const c_void) -> i64;
    /// torajs-meta `classmeta` — the registered template class cell.
    fn __torajs_class_cell_raw(tag: i64) -> u64;
}

/// `Some(instance)` when `callee` is a unit-class object whose template
/// factory has a boxed entry; `None` leaves the miss to the caller.
///
/// # Safety
/// `callee` boxes a live dynobj cell; `argv` points at `argc` readable
/// AnyValue slots.
pub(crate) unsafe fn construct_unit(
    callee: AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let tag = unsafe { __torajs_class_unit_tag_of(as_void_ptr(callee) as *const c_void) };
    if tag < 0 {
        return None;
    }
    let entry = crate::construct::ctor_entry(unsafe { __torajs_class_cell_raw(tag) });
    if entry == 0 {
        return None;
    }
    let mut args: Vec<u64> = Vec::with_capacity(argc as usize + 1);
    args.push(callee);
    if argc > 0 {
        // SAFETY: the caller's `argc` readable slots.
        args.extend_from_slice(unsafe { core::slice::from_raw_parts(argv, argc as usize) });
    }
    Some(unsafe { invoke_boxed(core::ptr::null_mut(), entry, args.as_ptr(), argc + 1) })
}
