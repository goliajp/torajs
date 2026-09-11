//! §7.4.6 IteratorNext's dispatch half — invoking a zero-argument
//! method on a `Tag::Obj` receiver through the class-methods vtable.
//!
//! Split from [`crate::iter_any`] at the size cap. The seam is the
//! question each side answers: this file answers "what happened when
//! we called `next` / `return` / `[Symbol.iterator]` on this
//! instance", the cascade next door answers "which lane does this
//! receiver iterate on".

use core::ffi::c_void;

use crate::method_call::invoke_boxed;
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-throw — non-zero iff a throw is in flight. Every method
    /// reached from here runs USER code (a generator body, a custom
    /// `next()`), so each call has to be checked before its result is
    /// touched.
    fn __torajs_throw_check() -> i64;
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — class-method boxed adapter by name bytes
    /// (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
}

/// `class_tag` u32 offset inside a `Tag::Obj` instance — mirror of
/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// What came back from [`call_obj_method_0`].
pub(crate) enum MethodOutcome {
    /// The receiver has no layout, or declares no method by that name.
    Missing,
    /// The method ran and threw. Its "result" is the sentinel an
    /// aborted fn returns — NOT a value: it must not be read, released,
    /// or handed on. The pending throw is left in flight for the
    /// caller's own throw-check to propagate.
    Threw,
    Ok(AnyValue),
}

/// Invoke a zero-argument method on a `Tag::Obj` receiver through the
/// class-methods dispatch table.
///
/// Every method reached from this module is USER code — a generator
/// body, a hand-written `next()` — so it can throw, and ES §7.4.6
/// IteratorNext forwards that abrupt completion rather than looking at
/// a result. Reading the sentinel as an IteratorResult instead is a
/// wild deref: `for (const v of gen)` over a generator that throws on
/// its first step was a SIGSEGV, and the destructuring lanes silently
/// clobbered the thrown Error (`e.message` came back empty).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn call_obj_method_0(obj: *mut c_void, name: &[u8]) -> MethodOutcome {
    let class_tag = unsafe { obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return MethodOutcome::Missing;
    }
    let adapter = unsafe { __torajs_struct_method_find(layout, name.as_ptr(), name.len() as u32) };
    if adapter.is_null() {
        return MethodOutcome::Missing;
    }
    // argc 0 — invoke_boxed hands the adapter its undefined-filled
    // argv buffer, so a defaulted param (a generator `next`'s
    // `__yield_arg`) materializes its own default.
    let argv: [u64; 0] = [];
    let result = unsafe { invoke_boxed(obj, adapter as u64, argv.as_ptr(), 0) };
    if unsafe { __torajs_throw_check() } != 0 {
        return MethodOutcome::Threw;
    }
    MethodOutcome::Ok(result)
}
