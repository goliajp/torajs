//! 563-06 / 564-03 — a builtin constructor reads as the class it is.
//!
//! `console.log(Map)` printed `[Function: Map]` because a builtin
//! ctor value is an interned `Tag::Closure` cell
//! (`method_value/ctor.rs`), and every closure went down the
//! `[Function: <name>]` arm. bun asks JSC whether the callee is a
//! class constructor and prints `[class Map]`; the twelve typed
//! arrays carry their `%TypedArray%` parent
//! (`[class Uint8Array extends TypedArray]`), and `Promise` is the
//! one builtin JSC writes as a plain function, so it keeps
//! `[Function: Promise]`.
//!
//! The same question is asked from the other side of the crate
//! seam: `class A extends Object {}` prints
//! `[class A extends Object]`, so torajs-meta's class-object
//! walker must be able to name a parent that is one of these cells
//! rather than a registered class object. Both faces read the one
//! predicate here — which is why `class P extends Promise {}`
//! prints `[class P]` with no `extends` in bun, and does here too.

use core::ffi::c_void;

use super::formatters::put_bytes;

/// `Promise`'s slot in the `torajs_rc::builtin_proto` ctor table —
/// the one builtin whose print face is the function form.
const PROMISE_CTOR_TAG: i64 = 10;

/// The `%TypedArray%` subclasses' slots in that same table
/// (`Int8Array` .. `Float16Array`).
const TYPED_ARRAY_CTOR_TAGS: core::ops::RangeInclusive<i64> = 20..=31;

/// The ctor tag of a cell that reads as a class, `None` for every
/// other pointer — including the `Promise` cell, which is a builtin
/// constructor but not a class.
fn class_form_tag(cell: *const c_void) -> Option<i64> {
    let tag = crate::method_value::ctor_tag_of_cell(cell)?;
    if tag == PROMISE_CTOR_TAG {
        return None;
    }
    Some(tag)
}

/// The interned `.name` Str cell of a builtin ctor cell that reads
/// as a class, NULL otherwise — torajs-meta's class-object walker
/// names an `extends` parent through this (564-03).
///
/// # Safety
/// `cell` is NULL or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_ctor_class_name(cell: *const c_void) -> *const c_void {
    let Some(tag) = class_form_tag(cell) else {
        return core::ptr::null();
    };
    match crate::method_value::ctor::ctor_name_cell(tag) {
        Some(p) => p as *const c_void,
        None => core::ptr::null(),
    }
}

/// Emit a builtin constructor's `[class <name>]` form and answer
/// true; answer false for every other closure, leaving the caller's
/// `[Function: <name>]` arm.
///
/// # Safety
/// `closure` is a live `Tag::Closure` cell.
pub(super) unsafe fn put_ctor_class_form(closure: *const c_void) -> bool {
    let Some(tag) = class_form_tag(closure) else {
        return false;
    };
    let Some((name, _)) = torajs_rc::builtin_proto::builtin_ctor_meta(tag) else {
        return false;
    };
    unsafe {
        put_bytes(b"[class ");
        put_bytes(name.as_bytes());
        if TYPED_ARRAY_CTOR_TAGS.contains(&tag) {
            // %TypedArray% has no global binding of its own, so this
            // is the one parent named by intrinsic rather than by a
            // reachable value.
            put_bytes(b" extends TypedArray");
        }
        put_bytes(b"]");
    }
    true
}
