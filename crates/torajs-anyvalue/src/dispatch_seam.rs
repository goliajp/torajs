//! The arm-seam族 declarations (RFC 20260824-s2-5 Phase B blade 2a).
//!
//! Fifteen per-family dispatch arms, all sharing one C-ABI shape.
//! The default definitions live in `torajs-dispatch` (a separate
//! archive member forwarding to [`crate::dispatch_arms`]); being
//! `extern "C"` declarations in THIS crate makes every skeleton
//! call site a true undef reloc, so a compiler-emitted loud-reject
//! stub in the user `.o` can shadow an unused family's arm and the
//! family kernel dead-strips (user definitions win the vaddr table
//! and the member closure — see `torajs-link::fn_addr_syms`).
//!
//! Safety contract shared by all fifteen: `recv` boxes a live value
//! whose shape matches the family (the skeleton's tag ladder is the
//! router); `name_str` is NULL for a reified-cell re-dispatch;
//! `recv_slot` is NULL or the receiver variable's live slot; `argv`
//! holds `argc` live AnyValue slots.

use crate::nanbox::AnyValue;

macro_rules! declare_arms {
    ($($name:ident),+ $(,)?) => {
        unsafe extern "C" {
            $(
                pub(crate) fn $name(
                    recv: AnyValue,
                    mid: i64,
                    name_str: *const u8,
                    recv_slot: *mut u64,
                    argv: *const u64,
                    argc: i64,
                ) -> AnyValue;
            )+
        }
    };
}

unsafe extern "C" {
    /// The ns-static MINT seam (S2-5 ns-world narrowing): every
    /// in-crate site that mints a namespace-static cell calls this
    /// declaration; `torajs-dispatch` owns the default definition
    /// (forwarding to
    /// [`crate::method_value::ns_static_cell_impl`]), so a
    /// compiler-emitted loud-reject stub in the user `.o` can
    /// shadow it and the whole ns-static dispatch universe
    /// (`ns_dispatch_entry` + the DISPATCH table + every static's
    /// kernel) dead-strips for programs that provably never put a
    /// namespace / ctor / globalThis object into the any world.
    pub(crate) fn __torajs_ns_static_cell(id: i64) -> *mut u8;
}

unsafe extern "C" {
    /// The Date-as-a-function arm of the builtin-ctor call entry
    /// (§21.4.2 — arguments ignored, answers the current time as a
    /// string). Split behind its own seam because it is the only
    /// ctor-call arm whose kernel world (torajs-date + the timezone
    /// tables) is not already kept by the coercion keep — stubbed
    /// together with the date family arm, since a Date ctor cell
    /// cannot be read off any receiver in a program with no date
    /// values.
    pub(crate) fn __torajs_ctor_date_call() -> u64;
}

declare_arms!(
    __torajs_dispatch_str_arm,
    __torajs_dispatch_arr_arm,
    __torajs_dispatch_dynobj_arm,
    __torajs_dispatch_struct_arm,
    __torajs_dispatch_mapset_arm,
    __torajs_dispatch_iter_arm,
    __torajs_dispatch_buffer_arm,
    __torajs_dispatch_date_arm,
    __torajs_dispatch_promise_arm,
    __torajs_dispatch_regexp_arm,
    __torajs_dispatch_bigint_arm,
    __torajs_dispatch_symbol_arm,
    __torajs_dispatch_closure_arm,
    __torajs_dispatch_weak_arm,
    __torajs_dispatch_num_arm,
);
