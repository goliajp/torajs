//! Builtin-prototype-singleton own-face probe for the HasOwnProperty
//! substrate — carved out of `prop_has.rs` (file-size hard limit)
//! when RFC 20260721 刀 11 G11 grew the `constructor` arm past 500
//! prod lines. Verbatim move; `prop_has`'s DynObj and Arr arms are
//! the callers.

use core::ffi::c_void;

use crate::prop_has::key_is;

/// Own-property probe for a dynobj that turns out to be a builtin
/// `<Ctor>.prototype` singleton: the interned method families count
/// as own entries (`proto_tag_owns` — the universal probes stay
/// Object.prototype-only). Answers 0 for every ordinary dynobj.
pub(crate) unsafe fn builtin_proto_method_own(ptr: *const c_void, key: *const c_void) -> i64 {
    let tag = unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_tag_of(ptr) };
    if tag < 0 {
        return 0;
    }
    let mid = unsafe { crate::method_value::key_method_id(key) };
    if mid == torajs_rc::ANY_METHOD_UNKNOWN {
        // `constructor` is an own data property of every builtin
        // prototype (§20.x.3.1 — RFC 20260721 刀 11 G11), virtual
        // like the method families; shaded by its tombstone slot.
        if unsafe { key_is(key, b"constructor") } {
            let deleted = unsafe {
                torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(
                    tag,
                    torajs_rc::ANY_METHOD_CONSTRUCTOR_SLOT,
                )
            };
            return (deleted == 0) as i64;
        }
        // The Map/Set `size` accessor is an own property too — it
        // deliberately doesn't intern (C2-size), so probe it here
        // behind the same tombstone gate.
        if let Some(amid) = unsafe { crate::method_support::proto_tag_accessor_mid(tag, key) } {
            let deleted =
                unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, amid) };
            return (deleted == 0) as i64;
        }
        return 0;
    }
    crate::method_support::proto_tag_owns(tag, mid) as i64
}
