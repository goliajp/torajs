//! Unit-test doubles for torajs-rc's builtin prototype singletons: no
//! unit test mints one, so each answers as the runtime does before the
//! first mint.

// Faithful no-op double (the real note is a no-op too until some
// `<Ctor>.prototype` singleton is minted, which no unit test does).
/// # Safety
/// Test stub; dereferences none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_note_key(
    _obj: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
) {
}

// Faithful double for the same reason: no unit test mints a
// `<Ctor>.prototype` singleton, so the real probe answers 0 here too.
/// # Safety
/// Test stub; dereferences none of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_builtin_proto_own_method_cell(
    _obj: *const core::ffi::c_void,
    _key: *const core::ffi::c_void,
) -> u64 {
    0
}

/// Test stub for torajs-rc's builtin prototype slots: nothing is
/// minted in a unit test.
#[unsafe(no_mangle)]
pub static __torajs_builtin_proto_slots: [core::sync::atomic::AtomicUsize; 2] =
    [const { core::sync::atomic::AtomicUsize::new(0) }; 2];
