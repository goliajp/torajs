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

unsafe extern "C" {
    /// The RegExp protocol seam: the §22.2.6 protocol algorithms and
    /// the RegExp String Iterator step, behind the RegExp arm (the
    /// iterator step and a method cell's object `this` call it
    /// directly); its default definition is `torajs-regexp-lane`.
    /// Split out because a program can hold regexes without ever calling a
    /// protocol method through the any world (every typed
    /// `s.replace(re, …)` lowers to a kernel), and the algorithms are
    /// a fifth of the regex family's text; the dispatch judgment stubs
    /// this seam in a program that never enters the RegExp world, and
    /// in one with no protocol method id and no string symbol-dispatch
    /// gate.
    pub(crate) fn __torajs_regexp_protocol_call(
        recv: AnyValue,
        mid: i64,
        argv: *const u64,
        argc: i64,
    ) -> AnyValue;
}

unsafe extern "C" {
    /// §22.2.3.1 RegExpInitialize for a first-class RegExp constructor
    /// value (`R(p, f)` / `new R(p, f)`): the shared build body of the
    /// ctor-call and ctor-construct entries. Behind its own seam
    /// because those two entries are linked into every program that
    /// calls or constructs any builtin ctor value, while a RegExp ctor
    /// cell exists only in a program that reaches the RegExp world;
    /// the dispatch judgment stubs it, with the protocol seam, in every
    /// other program, and the pattern compiler dead-strips with it.
    pub(crate) fn __torajs_ctor_regexp_build(pattern: AnyValue, flags: AnyValue) -> AnyValue;
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
