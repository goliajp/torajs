//! Reflect.* namespace-static detached-call arms (rotation 266 刀
//! R1/R2) — split from `ns_static_ctor.rs` under the 500-line file
//! rule. Every arm shares the same shape: the §10.1.6.3 strict
//! IsObject gate (`__torajs_anyv_throw_typeerror_if_not_object` —
//! every primitive throws, no ToObject) in front of the Object-flavor
//! kernel the typed lowering also consumes.
//!
//! MAINTENANCE: externs added here need matching no-op stubs in
//! `lib.rs`'s `#[cfg(test)] mod tests` (the table is test-reachable;
//! see `ns_static_table.rs`'s note).

use crate::nanbox::{VALUE_FALSE, VALUE_TRUE, VALUE_UNDEFINED};

use super::ns_static::arg_at;
use super::ns_static_ctor::gopd_static;
use super::ns_static_table::{
    __torajs_anyv_get_proto_of_any, __torajs_anyv_is_extensible, __torajs_anyv_prevent_extensions,
    __torajs_throw_check,
};

unsafe extern "C" {
    /// torajs-meta — the §10.1.6.3 strict Type(O) gate (every
    /// primitive throws, heap-cell Str/BigInt/Symbol included).
    fn __torajs_anyv_throw_typeerror_if_not_object(obj_any: u64);
}

/// §28.1.5 Reflect.getOwnPropertyDescriptor as a detached call —
/// step 1 is a **strict** IsObject check (every primitive throws, no
/// ToObject wrap, unlike the §20.1.2.8 Object flavor); a passing
/// target rides the same ToString(P) + descriptor-kernel path as
/// [`gopd_static`].
pub(super) unsafe fn reflect_gopd_static(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        gopd_static(argv, argc)
    }
}

/// §28.1.4 Reflect.getPrototypeOf as a detached call — strict gate,
/// then the same proto-classifier kernel as `Disp::ObjectGetProtoOf`.
pub(super) unsafe fn reflect_get_proto(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_get_proto_of_any(arg_at(argv, argc, 0))
    }
}

/// §28.1.10 Reflect.preventExtensions as a detached call — strict
/// gate, flip the header flag, answer boolean `true` (ordinary
/// [[PreventExtensions]] always succeeds). The kernel's un-inc'd
/// receiver pass-through is deliberately dropped without a dec — it
/// aliases the caller's argument, no ref transferred.
pub(super) unsafe fn reflect_prevent_extensions(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let _ = __torajs_anyv_prevent_extensions(arg_at(argv, argc, 0));
        VALUE_TRUE
    }
}

/// §28.1.8 Reflect.isExtensible as a detached call — strict gate +
/// the header-flag reader.
pub(super) unsafe fn reflect_is_extensible(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if __torajs_anyv_is_extensible(arg_at(argv, argc, 0)) {
            VALUE_TRUE
        } else {
            VALUE_FALSE
        }
    }
}
