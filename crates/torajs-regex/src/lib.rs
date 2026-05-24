//! Regex substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate, P6.2 — replaces `runtime_regex.c` (3059 LOC).
//! The port ships in substeps:
//!
//! - **P6.2-a (this commit)** — kernel modules: [`utf8`], [`ucd`],
//!   [`charclass`], [`node`]. No extern "C" surface yet.
//! - **P6.2-b** — parser (recursive-descent over pattern bytes).
//! - **P6.2-c** — compiler + flags + resolve_backrefs (Thompson NFA).
//! - **P6.2-d** — cutover `compile / get_source / drop` extern API.
//! - **P6.2-e** — VM + `regex_test / find / str_match_regex`.
//! - **P6.2-f** — replace + split + exec + matchAll + nuke C file.
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`utf8`] — `utf8_len_for / encode_cp / decode_cp`. Used by parser
//!   (for `\u{HHHH}` escape) and VM (for u-flag `.` advance).
//! - [`ucd`] — curated UCD Letter/Number ranges + binary-search
//!   membership. Powers `\p{L}` / `\p{N}` under the u flag.
//! - [`charclass`] — 256-bit ASCII bitmap + inversion bit + Unicode
//!   property bitfield + add/test primitives. One per `OP_CLASS`
//!   instruction in the future Program.
//! - [`node`] — regex AST node kinds + struct + ctor. Memory ownership
//!   is `Vec<Box<Node>> + Option<Box<Node>>` — Rust's Drop recursively
//!   frees the tree (replaces C's manual `node_free`).

pub mod charclass;
pub mod compiler;
pub mod flags;
pub mod node;
pub mod parser;
pub mod program;
pub mod regex;
pub mod resolve;
pub mod ucd;
pub mod utf8;
pub mod vm;

// Cross-tier extern "C" stubs for cargo unit tests — real symbols
// live in sibling staticlibs (torajs-rc, torajs-str, torajs-arr,
// torajs-dynobj, torajs-throw) at `tr build` link time. cargo test
// for torajs-regex doesn't link those, so panicking stubs keep the
// test binary linking clean. Same pattern as torajs-promise /
// torajs-cycle / torajs-weak / torajs-collections test stubs.

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    panic!("torajs-regex test stub: __torajs_rc_dec should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!(
        "torajs-regex test stub: __torajs_str_alloc_pooled should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_drop(_s: *mut core::ffi::c_void) {
    panic!("torajs-regex test stub: __torajs_str_drop should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_alloc(_cap: u64) -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_arr_alloc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push(
    _arr: *mut core::ffi::c_void,
    _val: i64,
) -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_arr_push should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_alloc() -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_dynobj_alloc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_set(
    _obj_slot: *mut *mut core::ffi::c_void,
    _key: *mut core::ffi::c_void,
    _tag: u64,
    _value: u64,
) {
    panic!("torajs-regex test stub: __torajs_dynobj_set should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arrprops_set(
    _arr_ptr: *mut core::ffi::c_void,
    _key: *mut core::ffi::c_void,
    _tag: i64,
    _value: i64,
) {
    panic!("torajs-regex test stub: __torajs_arrprops_set should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_type_error(_msg: *const u8) {
    panic!(
        "torajs-regex test stub: __torajs_throw_type_error should not be called from cargo test"
    );
}
