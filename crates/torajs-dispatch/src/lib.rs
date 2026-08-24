//! The `__torajs_any_method_dispatch` link seam (RFC
//! 20260824-s2-5 selective registration, Phase B blade 0).
//!
//! `torajs-anyvalue`'s `any_method_call_inner` / redispatch reach
//! the dispatcher through an `extern "C"` declaration; this crate
//! owns the default definition as a SEPARATE staticlib member so:
//!
//! - normal links resolve the seam here and forward to the
//!   monolithic [`torajs_anyvalue::any_method_dispatch_impl`];
//! - a compiler-emitted specialized dispatcher in the user `.o`
//!   shadows this member (user definitions win the member closure),
//!   and the monolith — whose only reference is the forward below —
//!   dead-strips together with every family arm the program never
//!   uses.
//!
//! Keep this crate a single thin forwarder: any logic added here
//! becomes logic a specialized dispatcher must replicate.

#![no_std]

/// Default (monolithic) resolution of the dispatch seam.
///
/// # Safety
/// Same contract as `__torajs_any_method_call`: cell receivers are
/// valid heap pointers; `argv` holds `argc` live AnyValue slots;
/// `recv_slot` is NULL or the receiver variable's live slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_method_dispatch(
    recv: u64,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
    skip_wrapper_expando: bool,
) -> u64 {
    unsafe {
        torajs_anyvalue::any_method_dispatch_impl(
            recv,
            mid,
            name_str,
            recv_slot,
            argv,
            argc,
            skip_wrapper_expando,
        )
    }
}
