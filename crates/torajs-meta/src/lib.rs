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
//!   `__proto_<C>` and `__class_<C>` AnyValue immediates.
//!   Lifetime-of-process references; no rc bump on register
//!   (caller's let binding keeps the box alive).
//! - [`reflect`] — `Object.getPrototypeOf(any)` and
//!   `Object.getOwnPropertyDescriptor(obj, key)` reflection helpers.
//!
//! Cross-tier extern symbols resolved at `tr build` link time:
//! - `__torajs_dynobj_alloc / set / has / get_tag / get_value /
//!   get_flags` — `torajs-dynobj`
//! - `__torajs_anyv_*` family — `torajs-anyvalue`
//! - `__torajs_rc_inc` — `torajs-rc`
//! - `__torajs_str_alloc_pooled / str_drop` — `torajs-str`
//! - `__torajs_value_drop_heap` — `runtime_str.c` (will move in P7.i)

pub(crate) mod arr_reflect;
pub mod classmeta;
pub(crate) mod closure_reflect;
pub mod error_to_string;
pub mod extensible_reflect;
pub mod fnprops;
pub mod from_entries;
pub mod genfn;
pub mod iterator_proto;
pub mod map_group_by;
pub mod obj_assign;
pub mod obj_forin_keys;
pub mod obj_own_descriptors;
pub mod obj_own_keys;
pub(crate) mod obj_own_keys_key_shape;
mod obj_own_keys_layout;
pub(crate) mod obj_own_keys_struct;
pub mod obj_own_values;
pub mod object_group_by;
pub mod object_proto_install;
pub mod own_names;
pub mod proto_tostringtag_install;
pub mod reflect;
pub mod reflect_descriptors;
pub mod reflect_get_property_descriptor;
pub mod reflect_proto;
pub mod reflect_proto_set;
pub mod str_descriptor;
pub mod string_raw;
pub mod struct_enum;
pub mod struct_field_attrs;
pub mod struct_print;
pub mod struct_reflect;
pub mod subclass_instance;
pub mod throw_readonly;

pub use classmeta::{
    __torajs_anyv_class_get, __torajs_anyv_class_register, __torajs_anyv_proto_get,
    __torajs_anyv_proto_register,
};
pub use error_to_string::__torajs_error_to_string;
pub use fnprops::{
    __torajs_fnprops_bind_cell, __torajs_fnprops_get_tag, __torajs_fnprops_get_value,
    __torajs_fnprops_set,
};
pub use reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor;
pub use reflect_proto::{__torajs_anyv_get_proto_of_any, __torajs_anyv_proto_member_get};
pub use throw_readonly::__torajs_throw_readonly_assign;

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
pub unsafe extern "C" fn __torajs_builtin_method_cell(_mid: i64) -> *mut u8 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_method_cell should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_pair_new(
    _get: *mut core::ffi::c_void,
    _set: *mut core::ffi::c_void,
    _kinds: u64,
) -> *mut core::ffi::c_void {
    panic!(
        "torajs-meta test stub: __torajs_accessor_pair_new should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define(
    _dst: *mut *mut core::ffi::c_void,
    _key: *const u8,
    _tag: u64,
    _value: u64,
    _flags_byte: u64,
) {
    panic!("torajs-meta test stub: __torajs_dynobj_define should not be called from cargo test");
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

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_method_cell(
    _dynobj: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
) -> u64 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_proto_own_method_cell should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ctor_wellknown_symbol(
    _cell: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
) -> *mut core::ffi::c_void {
    panic!(
        "torajs-meta test stub: __torajs_ctor_wellknown_symbol should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_meta(
    _dynobj: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
    _out_tag: *mut u64,
    _out_val: *mut u64,
) -> i64 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_proto_own_meta should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_get_last_index(_re: *const core::ffi::c_void) -> f64 {
    panic!(
        "torajs-meta test stub: __torajs_regex_get_last_index should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_last_index_raw(_re: *const core::ffi::c_void) -> u64 {
    panic!(
        "torajs-meta test stub: __torajs_regex_last_index_raw should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_accessor_getter(
    _dynobj: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
) -> u64 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_proto_own_accessor_getter should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_get_builtin_prototype(_tag: i64) -> *mut core::ffi::c_void {
    panic!(
        "torajs-meta test stub: __torajs_get_builtin_prototype should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_tag_of(_p: *const core::ffi::c_void) -> i64 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_proto_tag_of should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_name_str(_p: *mut core::ffi::c_void) -> *mut u8 {
    panic!("torajs-meta test stub: __torajs_closure_name_str should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_length(_p: *mut core::ffi::c_void) -> i64 {
    panic!("torajs-meta test stub: __torajs_closure_length should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_is_deleted(_tag: i64, _mid: i64) -> i64 {
    panic!(
        "torajs-meta test stub: __torajs_builtin_proto_is_deleted should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_tag(
    _recv: u64,
    _key: *const core::ffi::c_void,
) -> u64 {
    panic!(
        "torajs-meta test stub: __torajs_any_member_get_tag should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_value(
    _recv: u64,
    _key: *const core::ffi::c_void,
) -> u64 {
    panic!(
        "torajs-meta test stub: __torajs_any_member_get_value should not be called from cargo test"
    );
}
