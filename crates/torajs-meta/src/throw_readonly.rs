//! Throw helper for writing a property that has a [[Get]] but no
//! [[Set]] (ES §10.1.9 step 4.b — `OrdinarySetWithOwnDescriptor`
//! returns false, and a strict-mode write of a false-returning [[Set]]
//! is a `TypeError` per §13.15.2). A module is always strict, so tr
//! reaches this from every get-only write it can see.
//!
//! `Object.assign(target, source)` walks the target through [[Set]]
//! (§20.1.2.1 step 4.c.ii.2), so a get-only target property throws
//! there too — the one site that knows this AT COMPILE TIME (the
//! target's layout carries `__getter_v` with no `__setter_v` half),
//! which is why this is a 0-arg helper the SSA arm emits
//! unconditionally rather than a runtime shape test.
//!
//! Same shape as `torajs-arr`'s `throw_empty` helpers: record the
//! pending throw via `__torajs_throw_type_error` and return normally —
//! the call site's `emit_throw_check(None)` propagates it to the
//! nearest catch or to `__torajs_uncaught_exit_code`.

use core::ffi::c_char;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// The message matches the `any`-lane member-write kernel
/// (`torajs-anyvalue`'s `member_set`), which bun words identically.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_readonly_assign() {
    // SAFETY: NUL-terminated static C string.
    unsafe {
        __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
    }
}
