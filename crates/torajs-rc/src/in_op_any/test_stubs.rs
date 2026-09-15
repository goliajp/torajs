//! Cargo-test stubs for the NaN-box / prop_has / chain externs. Real
//! symbols live in torajs-anyvalue / torajs-dynobj / torajs-num; unit
//! tests here hand-roll a NaN-box + result thread-locals so the
//! kernel's dispatch (rhs gate → own face → chain face) verifies in
//! isolation. Own-face and chain-face INTERNALS are covered by their
//! home crates' tests + the conformance fixtures.
//!
//! Moved out of the parent (rotation 655, file-size): test scaffolding
//! the parent re-imports under `cfg(test)`, where its extern block is
//! compiled out.

use core::ffi::c_void;

use super::ANY_TAG_HEAP;

pub(super) unsafe fn __torajs_anyv_unbox_tag(v: i64) -> i64 {
    // Faithful to the real encoding for the cell shape: a heap box
    // IS the raw pointer (top16 zero), which is what lets the
    // kernel's `nan_box_is_cell_like` gate run against test values.
    // Non-cell mock shapes keep the legacy `tag << 48` spelling.
    if crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return ANY_TAG_HEAP;
    }
    ((v as u64 >> 48) & 0xFFFF) as i64
}

pub(super) unsafe fn __torajs_anyv_unbox_value(v: i64) -> i64 {
    if crate::ffi::nan_box_is_cell_like(v as u64 as *mut c_void) {
        return v;
    }
    (v as u64 & 0x0000_FFFF_FFFF_FFFF) as i64
}

pub(super) unsafe fn __torajs_dynobj_has(_obj: *const c_void, _key: *const u8) -> i32 {
    // The num kernel's Arr hole probe is exotic-index-gated; tests
    // never set the flag, so this stays unreached there.
    0
}

pub(super) unsafe fn __torajs_dynobj_entry_is_hole(_obj: *const c_void, _key: *const u8) -> i32 {
    0
}

pub(super) unsafe fn __torajs_any_prop_has(_recv: u64, _key: *const c_void) -> i64 {
    PROP_HAS_RESULT.with(|r| r.get())
}

pub(super) unsafe fn __torajs_any_has_property(_recv: u64, _key: *const c_void) -> i64 {
    PROP_HAS_RESULT.with(|r| r.get())
}

pub(super) unsafe fn __torajs_builtin_ctor_parent_value(_cell: *const c_void) -> u64 {
    // Unit tests build no builtin constructor cells.
    0
}

pub(super) unsafe fn __torajs_dynobj_user_proto(_dynobj: *const c_void) -> *mut c_void {
    // Unit tests never build a user [[Prototype]] chain — the chain
    // hop is covered by the conformance fixtures.
    core::ptr::null_mut()
}

pub(super) unsafe fn __torajs_proto_chain_key_owned(family_tag: i64, _key: *const c_void) -> i64 {
    CHAIN_SEEN_FAMILY.with(|f| f.set(family_tag));
    CHAIN_RESULT.with(|r| r.get())
}

pub(super) unsafe fn __torajs_struct_proto_member_has(
    _ptr: *const c_void,
    _key: *const c_void,
) -> i64 {
    STRUCT_PROTO_RESULT.with(|r| r.get())
}

pub(super) unsafe fn __torajs_buffer_family_proto_key(
    _recv: *const c_void,
    _tag: i64,
    _key: *const c_void,
) -> i64 {
    // Unit tests never build a buffer-family receiver.
    0
}

pub(super) unsafe fn __torajs_throw_type_error(_msg: *const u8) {
    // The real symbol records a pending throw in TLS for ssa_lower's
    // emit_throw_check to propagate; here we only need to observe
    // that the rhs gate fired, since the return value alone cannot
    // tell "not an own property" from "not an object".
    THROWN.with(|c| c.set(c.get() + 1));
}

thread_local! {
    pub(super) static THROWN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    pub(super) static PROP_HAS_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
    pub(super) static CHAIN_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
    pub(super) static CHAIN_SEEN_FAMILY: core::cell::Cell<i64> = const { core::cell::Cell::new(-1) };
    pub(super) static STRUCT_PROTO_RESULT: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
}

pub(super) fn thrown_by(f: impl FnOnce() -> bool) -> (bool, bool) {
    THROWN.with(|c| c.set(0));
    let r = f();
    (r, THROWN.with(|c| c.get()) > 0)
}

// Test stub for the cross-crate num→str alloc. Returns a static-
// buffer pointer the stubs above never deref-read. Pairs with
// rc_dec_temp_str's cfg(test) no-op so the mock pointer never enters
// the drop-dispatch path.
pub(super) static TEST_NUM_TO_STR_BUF: [u8; 16] = [0u8; 16];

pub(super) unsafe fn __torajs_num_to_string_radix_i(_n: i64, _radix: i64) -> *mut u8 {
    TEST_NUM_TO_STR_BUF.as_ptr() as *mut u8
}
