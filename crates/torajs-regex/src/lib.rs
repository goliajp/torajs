//! Regex substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate, P6.2 — replaced `runtime_regex.c` (3059 LOC),
//! deleted in `e481e99`. The port shipped P6.2-a kernel through the
//! P6.2-e/closer extern-API cutover (parser, Thompson-NFA compiler,
//! Pike-VM matcher, replace/split/matchAll).
//!
//! ## no_std (v0.7-A5, Step 16-e)
//!
//! The crate is `#![no_std]` + `extern crate alloc`. Its allocations
//! route through the active user binary's `#[global_allocator]`
//! (torajs-mmalloc, 16-d), and panics / alloc-errors land on
//! torajs-panic-runtime's active-mode handlers — so a `tr build`
//! binary exercising `.match()` / `.replace()` carries no libc
//! dependency (`nm -u` clean). Error paths write stderr + exit via
//! torajs-syscall (no libc `eprintln!` / `std::process`). cargo unit
//! tests pull `extern crate std` (libtest harness + std allocator).
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

#![no_std]

extern crate alloc;

// cargo unit tests need a panic_handler + global allocator + the
// libtest harness; in `tr build` user binaries those come from
// torajs-panic-runtime (active mode) + torajs-mmalloc at link time.
#[cfg(test)]
extern crate std;

/// Write `bytes` to stderr (fd 2) via a raw syscall — the no_std
/// replacement for `eprint!` on the regex error paths. Best-effort:
/// a short write or `EINTR` is ignored (these paths exit/abort next).
pub(crate) fn write_stderr(bytes: &[u8]) {
    unsafe {
        torajs_syscall::syscall3(
            torajs_syscall::sysno::SYS_WRITE,
            2,
            bytes.as_ptr() as i64,
            bytes.len() as i64,
        );
    }
}

pub mod charclass;
pub mod compiler;
pub mod cpset;
pub mod dfa;
pub mod flags;
pub mod node;
pub mod parser;
pub mod program;
pub mod regex;
pub mod resolve;
pub mod ucd;
pub mod ucd_emoji_seq;
pub mod ucd_tables;
pub mod utf8;
pub mod utf8_class_expand;
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
pub unsafe extern "C" fn __torajs_rc_inc(_p: *mut core::ffi::c_void) {
    panic!("torajs-regex test stub: __torajs_rc_inc should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer(_p: *mut core::ffi::c_void) {
    panic!("torajs-regex test stub: __torajs_cycle_buffer should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_unbuffer(_p: *mut core::ffi::c_void) {
    panic!("torajs-regex test stub: __torajs_cycle_unbuffer should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_drop_entry(_p: *mut core::ffi::c_void) {
    panic!(
        "torajs-regex test stub: __torajs_subclass_drop_entry should not be called from cargo test"
    );
}

// RFC 20260730 blade 2 — subclass mint/super kernel dep stubs
// (torajs-meta registry + torajs-anyvalue NaN-box faces).
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_subclass_register(_c: *mut core::ffi::c_void, _t: i64, _p: u64) {
    panic!("torajs-regex test stub: __torajs_subclass_register");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_cell_raw(_t: i64) -> u64 {
    panic!("torajs-regex test stub: __torajs_proto_cell_raw");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_box_from_pair(_tag: i64, _value: i64) -> u64 {
    panic!("torajs-regex test stub: __torajs_anyv_box_from_pair");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cell_ptr(_v: u64) -> i64 {
    panic!("torajs-regex test stub: __torajs_anyv_cell_ptr");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unbox_value(_v: u64) -> i64 {
    panic!("torajs-regex test stub: __torajs_anyv_unbox_value");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_str(_v: u64) -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_anyv_to_str");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_unbox_tag(_v: u64) -> i64 {
    panic!("torajs-regex test stub: __torajs_anyv_unbox_tag");
}
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_check() -> i64 {
    panic!("torajs-regex test stub: __torajs_throw_check");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_mark_null_proto(_obj: *mut core::ffi::c_void) {
    panic!(
        "torajs-regex test stub: __torajs_dynobj_mark_null_proto should not be called from cargo test"
    );
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
pub unsafe extern "C" fn __torajs_arr_alloc_any(_cap: u64) -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_arr_alloc_any should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_push_any(
    _arr: *mut core::ffi::c_void,
    _tag: u64,
    _value: u64,
) -> *mut core::ffi::c_void {
    panic!("torajs-regex test stub: __torajs_arr_push_any should not be called from cargo test");
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

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_syntax_error(_msg: *const u8) {
    panic!(
        "torajs-regex test stub: __torajs_throw_syntax_error should not be called from cargo test"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_to_number(_v: u64) -> f64 {
    panic!(
        "torajs-regex test stub: __torajs_anyv_to_number should not be called from cargo test (lastIndex stays in numeric form there)"
    );
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_child: *mut core::ffi::c_void) {
    panic!(
        "torajs-regex test stub: __torajs_value_drop_heap should not be called from cargo test (lastIndex stays in numeric form there)"
    );
}
