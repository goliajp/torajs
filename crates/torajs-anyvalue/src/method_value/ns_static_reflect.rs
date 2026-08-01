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
    /// torajs-meta — §28.1.12 boolean-answer OrdinarySetPrototypeOf
    /// (refusal = 0, no throw; invalid proto throws inside).
    fn __torajs_reflect_set_prototype_of(obj: u64, proto: u64) -> i64;
    /// torajs-meta — §6.2.6.5 step 1 gate (desc must be an object).
    fn __torajs_anyv_throw_typeerror_if_not_desc_object(desc_any: u64);
    /// torajs-dynobj — §28.1.2 boolean-answer runtime-descriptor
    /// define (refusal = 0, no throw; a ToPropertyDescriptor throw
    /// still records).
    fn __torajs_dynobj_define_from_desc_soft(
        obj_slot: *mut *mut core::ffi::c_void,
        key: *mut core::ffi::c_void,
        desc: *const core::ffi::c_void,
    ) -> i64;
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

/// §28.1.3 Reflect.deleteProperty as a detached call — strict gate,
/// ToString(P) key (the [`gopd_static`] posture; a symbol key rides
/// the wedge-lowered call face, not this cell), then the same
/// OrdinaryDelete kernel the `delete` expression lowers to.
pub(super) unsafe fn reflect_delete_property(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 1));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let deleted = crate::prop_delete::__torajs_any_prop_delete_soft(
            arg_at(argv, argc, 0),
            key as *const core::ffi::c_void,
        );
        crate::__torajs_str_drop(key as *mut core::ffi::c_void);
        if deleted != 0 {
            VALUE_TRUE
        } else {
            VALUE_FALSE
        }
    }
}

/// §28.1.2 Reflect.defineProperty as a detached call — strict gate
/// on the target, §6.2.6.5 desc-object gate, ToString(P) key (the
/// [`reflect_delete_property`] posture; a symbol key rides the
/// wedge-lowered call face, not this cell), then the boolean-answer
/// runtime-descriptor define kernel. The target cell rides a local
/// slot (the anyvalue member_set posture — a dynobj resize swaps the
/// cell within the kernel; the caller's box is not written back).
pub(super) unsafe fn reflect_define_property(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        __torajs_anyv_throw_typeerror_if_not_desc_object(arg_at(argv, argc, 2));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 1));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let mut target = crate::nanbox_encode::__torajs_anyv_cell_ptr(arg_at(argv, argc, 0))
            as *mut core::ffi::c_void;
        let desc = crate::nanbox_encode::__torajs_anyv_cell_ptr(arg_at(argv, argc, 2))
            as *const core::ffi::c_void;
        let ok =
            __torajs_dynobj_define_from_desc_soft(&mut target, key as *mut core::ffi::c_void, desc);
        crate::__torajs_str_drop(key as *mut core::ffi::c_void);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if ok != 0 { VALUE_TRUE } else { VALUE_FALSE }
    }
}

/// §28.1.12 Reflect.setPrototypeOf as a detached call — strict gate
/// + the boolean-answer OrdinarySetPrototypeOf core.
pub(super) unsafe fn reflect_set_prototype_of(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let ok = __torajs_reflect_set_prototype_of(arg_at(argv, argc, 0), arg_at(argv, argc, 1));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        if ok != 0 { VALUE_TRUE } else { VALUE_FALSE }
    }
}

/// §28.1.13 Reflect.set as a detached call — strict gate, ToString(P)
/// key (the [`gopd_static`] posture; a symbol key rides the
/// wedge-lowered call face, not this cell), then the [[Set]] kernel's
/// no-throw flavor. The owned unbox mints the payload +1 the entry
/// write consumes. The receiver rides a local slot (the
/// [`reflect_define_property`] posture — a dynobj resize swaps the
/// cell within the kernel; the caller's box is not written back).
pub(super) unsafe fn reflect_set(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        __torajs_anyv_throw_typeerror_if_not_object(arg_at(argv, argc, 0));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 1));
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let v = arg_at(argv, argc, 2);
        let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(v);
        let value = crate::nanbox_encode::__torajs_anyv_unbox_value_owned(v);
        let mut recv = arg_at(argv, argc, 0);
        let wrote = crate::member_set::__torajs_any_member_set_soft(
            &mut recv as *mut u64,
            key as *mut core::ffi::c_void,
            tag as u64,
            value as u64,
            -1,
        );
        crate::__torajs_str_drop(key as *mut core::ffi::c_void);
        if wrote != 0 { VALUE_TRUE } else { VALUE_FALSE }
    }
}
