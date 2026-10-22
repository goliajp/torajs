//! Default resolutions of the link-judged seams on the exit reporters'
//! Error rendering and on a handler's object return.

unsafe extern "C" {
    fn __torajs_uncaught_error_render_impl(p: *const u8);
    fn __torajs_unhandled_error_render_impl(p: *const u8);
    fn __torajs_anyv_promise_settle_thenable(
        result: *mut core::ffi::c_void,
        repr: i64,
        value: i64,
    ) -> i64;
}

/// Default resolution of the uncaught reporter's Error-instance
/// rendering (A6): `name: message` through the prototype-chain name
/// resolver in torajs-throw / torajs-anyvalue.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell carrying FLAG_ERROR.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uncaught_error_render_slow(p: *const u8) {
    unsafe { __torajs_uncaught_error_render_impl(p) }
}

/// Default resolution of the unhandled-rejection reporter's
/// Error-instance rendering, the uncaught one's twin (torajs-promise):
/// `name: message` through the same resolver.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell carrying FLAG_ERROR.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_unhandled_error_render_slow(p: *const u8) {
    unsafe { __torajs_unhandled_error_render_impl(p) }
}

/// Default resolution of the thenable probe on a handler's or an async
/// body's object return (§27.2.1.3.2 steps 8-9, torajs-anyvalue). The
/// link judgment stubs it (fam 63) when nothing can hand the settle an
/// object: no call site with an object or `any` return form (the
/// compiler's `__torajs_promise_obj_return_mark` root) and no handler
/// twin (`__torajs_then_twin_run`).
///
/// # Safety
/// `result` is a live pending cell; `value` carries the form `repr`
/// names, owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_settle_thenable_slow(
    result: *mut core::ffi::c_void,
    repr: i64,
    value: i64,
) -> i64 {
    unsafe { __torajs_anyv_promise_settle_thenable(result, repr, value) }
}
