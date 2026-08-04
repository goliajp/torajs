//! §27.2.4.2's all-rejected answer — the `AggregateError` a
//! `Promise.any` rejects with once every element has rejected, and the
//! `errors` list it carries.
//!
//! Until now `any` forwarded the LAST rejection reason instead. That
//! was the MVP posture, and it is observable in an ordinary way: a
//! `catch` reads `e.name` / `e.errors` and gets `undefined` off a bare
//! string, and `e instanceof AggregateError` is false.
//!
//! The class cannot be built here. `AggregateError` is a TS-level
//! class the compiler injects, so the runtime reaches it the way every
//! other runtime-raised error does — through torajs-throw's factory
//! registry, which `synthesize_module_init` fills with each Error
//! subclass's `__new_<C>`. This one is the registry's odd entry: its
//! factory carries the `errors` data param ahead of the shared
//! message, so torajs-throw reads that slot through its own typed
//! lookup and exposes the call as `__torajs_make_aggregate_error`.
//!
//! A program with no such class gets NULL back and the caller keeps
//! the old forwarding posture. That is not a fallback for its own
//! sake: `Promise.any` implies the injection (the same way bigint
//! division implies RangeError), so the only way to reach NULL is to
//! shadow the name with a user class — and then answering with a
//! user's unrelated constructor would be worse than answering with the
//! reason.

use core::ffi::c_void;

unsafe extern "C" {
    /// torajs-arr — `new Array(n)` any-shape alloc (len = cap = n,
    /// undefined-filled); the fill writes every slot it has a reason
    /// for and leaves the rest undefined.
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    /// torajs-anyvalue — NaN-box pack (tag 4 = heap cell).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-throw — invoke the registered `__new_AggregateError`,
    /// or NULL when no factory was registered.
    fn __torajs_make_aggregate_error(errors: i64) -> *mut c_void;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// The `errors` list: one slot per input element.
///
/// §27.2.4.2.1's reject-element functions store at `errors[index]`, so
/// the order is the INPUT's and not the order the elements settled in.
/// That is observable — bun answers `["e1", "e2"]` for a pair whose
/// second element rejects first — which is why the list is pre-sized
/// and written by index rather than pushed as reasons arrive.
pub(crate) unsafe fn alloc_errors(len: u64) -> *mut c_void {
    unsafe { __torajs_arr_alloc_any_filled(len) as *mut c_void }
}

/// Park one element's boxed reason in the slot it was given.
pub(crate) unsafe fn store_error(errors_arr: *mut c_void, index: u64, boxed: u64) {
    unsafe { crate::combinator_fanin_slot::store_slot(errors_arr, index, boxed as i64) };
}

/// Wrap a finished `errors` list in an `AggregateError`. Ownership of
/// the list transfers here either way; `None` means the program has no
/// class to build one from (see the module doc).
pub(crate) unsafe fn make(errors_arr: *mut c_void) -> Option<*mut c_void> {
    unsafe {
        let boxed = __torajs_anyv_box_from_pair(4, errors_arr as i64) as i64;
        let inst = __torajs_make_aggregate_error(boxed);
        // The ctor's `this.errors = errors` store takes its own
        // reference — the same convention torajs-throw's factory
        // dispatch relies on for the message Str — so the mint stake
        // is released here. On the NULL path nobody took it, and the
        // list dies with the attempt.
        __torajs_value_drop_heap(errors_arr);
        if inst.is_null() { None } else { Some(inst) }
    }
}
