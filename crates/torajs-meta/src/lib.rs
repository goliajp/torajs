//! Runtime metadata + reflection substrate for the torajs AOT
//! TypeScript runtime.
//!
//! Layer-3 substrate (P7.g, 2026-05-24) — replaces the
//! `fnprops` + `proto`/`class` registry + `get_property_descriptor` /
//! `get_proto_of_any` families in `runtime_str.c`. Three modules:
//!
//! - [`fnprops`] — `fn_ptr → dynobj` side table for `fn.x = v` on
//!   non-Closure functions (FnSig form). Closure-form fns use the
//!   in-layout `CLOSURE_PROPS_OFF` path.
//! - [`classmeta`] — class-tag-keyed fixed-256 arrays for the
//!   `__proto_<C>` and `__class_<C>` Any-boxes. Lifetime-of-process
//!   references; no rc bump on register (caller's let binding
//!   keeps the box alive).
//! - [`reflect`] — `Object.getPrototypeOf(any)` and
//!   `Object.getOwnPropertyDescriptor(obj, key)` reflection helpers.
//!
//! Cross-tier extern symbols resolved at `tr build` link time:
//! - `__torajs_dynobj_alloc / set / has / get_tag / get_value /
//!   get_flags` — `torajs-dynobj`
//! - `__torajs_any_box` — `torajs-anyvalue`
//! - `__torajs_rc_inc` — `torajs-rc`
//! - `__torajs_str_alloc_pooled / str_drop` — `torajs-str`
//! - `__torajs_value_drop_heap` — `runtime_str.c` (will move in P7.i)

pub mod classmeta;
pub mod fnprops;
pub mod reflect;

pub use classmeta::{
    __torajs_class_get, __torajs_class_register, __torajs_proto_get, __torajs_proto_register,
};
pub use fnprops::{__torajs_fnprops_get_tag, __torajs_fnprops_get_value, __torajs_fnprops_set};
pub use reflect::{__torajs_get_property_descriptor, __torajs_get_proto_of_any};

// ============================================================
// cargo-test stubs — cross-tier symbols
// ============================================================
//
// torajs-meta's unit tests run as a plain rlib + linked together
// for the test binary; the staticlib symbol from torajs-str /
// torajs-dynobj / torajs-anyvalue / runtime_str.c isn't available.
// Panicking stubs keep the test binary linking; tests in this
// crate only exercise pure-Rust logic (hash distribution,
// fixed-array bounds) that never reaches these symbols.

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-meta test stub: __torajs_str_alloc_pooled should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut u8) {
    panic!("torajs-meta test stub: __torajs_str_drop should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_box(_tag: i64, _value: i64) -> *mut core::ffi::c_void {
    panic!("torajs-meta test stub: __torajs_any_box should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!("torajs-meta test stub: __torajs_rc_inc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut core::ffi::c_void) {
    panic!("torajs-meta test stub: __torajs_value_drop_heap should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut core::ffi::c_void {
    panic!("torajs-meta test stub: __torajs_dynobj_alloc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set(
    _dst: *mut *mut core::ffi::c_void,
    _key: *const u8,
    _tag: u64,
    _value: u64,
) {
    panic!("torajs-meta test stub: __torajs_dynobj_set should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_has(
    _dynobj: *const core::ffi::c_void,
    _key: *const u8,
) -> bool {
    panic!("torajs-meta test stub: __torajs_dynobj_has should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_tag(
    _dynobj: *const core::ffi::c_void,
    _key: *const u8,
) -> u64 {
    panic!("torajs-meta test stub: __torajs_dynobj_get_tag should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_value(
    _dynobj: *const core::ffi::c_void,
    _key: *const u8,
) -> u64 {
    panic!("torajs-meta test stub: __torajs_dynobj_get_value should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_get_flags(
    _dynobj: *const core::ffi::c_void,
    _key: *const u8,
) -> u64 {
    panic!("torajs-meta test stub: __torajs_dynobj_get_flags should not be called from cargo test");
}
