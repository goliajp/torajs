//! Exotic-subclass method dispatch probe (RFC 20260730 blades 1-2).
//!
//! A `class C extends Array` (or Number / String / Boolean) instance
//! IS the exotic cell; its methods live in the class-methods dispatch
//! table under C's class tag, resolved through the blade-0 identity
//! side table. Each exotic arm calls this probe behind a
//! `FLAG_SUBCLASSED` gate (header already loaded — plain builtin
//! receivers pay one predicted-clear branch), AFTER the own-expando
//! shadow probe and BEFORE the builtin surface: C.prototype sits
//! between own properties and the builtin prototype on the spec
//! chain, so a user method (including an override of a builtin name)
//! wins exactly there.

use core::ffi::c_void;

use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-meta — blade-0 identity side table (-1 miss).
    fn __torajs_subclass_class_tag(cell: *const c_void) -> i64;
    /// torajs-structmeta — layout by class tag (NULL miss).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta 刀 4 — class-method boxed adapter by name
    /// bytes (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
}

/// Str header offsets (`torajs-str` layout mirror, same constants the
/// dynobj dispatcher carries).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// The class-method adapter the receiver's own class defines under
/// `name_str`, or `None` when the name is not one of them — the
/// RESOLUTION half of [`subclass_method`], split off because the
/// builtin-prototype pre-gate has to ask whether the subclass would
/// answer without calling it.
///
/// # Safety
/// `ptr` is a live exotic cell with `FLAG_SUBCLASSED` set; `name_str`
/// is a live Str cell.
pub(crate) unsafe fn subclass_adapter(
    ptr: *const c_void,
    name_str: *const u8,
) -> Option<*const c_void> {
    unsafe {
        let class_tag = __torajs_subclass_class_tag(ptr);
        if class_tag < 0 {
            return None;
        }
        let layout = __torajs_struct_layout_lookup(class_tag as u32);
        if layout.is_null() {
            return None;
        }
        let name_len = (name_str.add(STR_LEN_OFF) as *const u32).read();
        let name_bytes = name_str.add(STR_DATA_OFF);
        let adapter = __torajs_struct_method_find(layout, name_bytes, name_len);
        if adapter.is_null() {
            return None;
        }
        Some(adapter)
    }
}

/// Does the receiver's class define a method under `name_str`? The
/// pre-gate's question — `C.prototype` sits between the receiver's
/// own properties and the builtin prototype, so a patch on the
/// builtin prototype must not be consulted early where the subclass
/// is what a call would have resolved to.
///
/// # Safety
/// Same contract as [`subclass_adapter`].
pub(crate) unsafe fn subclass_owns(ptr: *const c_void, name_str: *const u8) -> bool {
    unsafe { subclass_adapter(ptr, name_str).is_some() }
}

/// Resolve + invoke a subclass method on an exotic receiver; `None`
/// falls through to the builtin surface (the name is not one of the
/// class's methods).
///
/// # Safety
/// `ptr` is a live exotic cell with `FLAG_SUBCLASSED` set; `name_str`
/// is a live Str cell (callers pass NULL-name mids straight to the
/// builtin arm); `argv`/`argc` follow the boxed-adapter convention.
pub(crate) unsafe fn subclass_method(
    ptr: *mut c_void,
    name_str: *const u8,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    unsafe {
        let adapter = subclass_adapter(ptr, name_str)?;
        Some(crate::method_call::invoke_boxed(
            ptr,
            adapter as u64,
            argv,
            argc,
        ))
    }
}

/// `super.<m>(args)` inside a builtin-heritage subclass method
/// (rotation 371) — §13.3.7.3 resolves the method on the PARENT
/// prototype, so the receiver's own override must NOT be consulted:
/// straight to the builtin re-dispatch (own-property probing is
/// over, the same contract as a reified cell's [[Call]]). A name
/// outside the builtin method id space answers the not-a-function
/// TypeError (the parent prototype has nothing callable there).
///
/// # Safety
/// `recv` is a live AnyValue; `argv` points at `argc` live slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_super_builtin_method(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if mid == 0 {
            __torajs_throw_type_error(c"super method is not a function".as_ptr());
            return crate::nanbox::VALUE_UNDEFINED;
        }
        crate::method_call::any_method_redispatch(recv, mid, argv, argc)
    }
}

unsafe extern "C" {
    /// torajs-throw — records a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}
