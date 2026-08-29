//! Builtin-prototype-singleton own-face probe for the HasOwnProperty
//! substrate — carved out of `prop_has.rs` (file-size hard limit)
//! when RFC 20260721 刀 11 G11 grew the `constructor` arm past 500
//! prod lines. Verbatim move; `prop_has`'s DynObj and Arr arms are
//! the callers.

use core::ffi::c_void;

use crate::prop_has::key_is;

/// For-in mid-loop liveness re-check for a TYPED Arr receiver (RFC
/// 20260721 刀 12 G16) — ES §14.7.5 forbids visiting a property
/// removed during enumeration, and a typed array can shrink
/// mid-loop (`a.length = 1` / `splice`). Routes the snapshot key
/// through the same §7.3.11 HasProperty walk the any-lane guard
/// uses (index-in-live-len + hole flags + expando + prototype
/// chain), with the receiver boxed borrow-style (the NaN-box pair
/// encode takes no reference).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_forin_key_live(arr: *mut c_void, key: *const c_void) -> i64 {
    unsafe {
        let boxed = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, arr as i64);
        crate::prop_has_chain::__torajs_any_has_property(boxed, key)
    }
}

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
        // prototype that HAS a constructor (§20.x.3.1 — RFC 20260721
        // 刀 11 G11), virtual like the method families and shaded by
        // its tombstone slot. `constructor_live` is the one home for
        // both halves; asking the tombstone here directly is what let
        // this face and the read one drift.
        if unsafe { key_is(key, b"constructor") } {
            return crate::method_support_proto::constructor_live(tag) as i64;
        }
        // The Map/Set `size` accessor is an own property too — it
        // deliberately doesn't intern (C2-size), so probe it here
        // behind the same tombstone gate.
        if let Some(amid) = unsafe { crate::method_support::proto_tag_accessor_mid(tag, key) } {
            let deleted =
                unsafe { torajs_rc::builtin_proto::__torajs_builtin_proto_is_deleted(tag, amid) };
            return (deleted == 0) as i64;
        }
        // Function.prototype's virtual own name/length pair
        // (§20.2.3, RFC 20260722 刀 3) — tombstone-gated inside the
        // meta probe.
        if unsafe { crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key) }.is_some() {
            return 1;
        }
        return 0;
    }
    crate::method_support::proto_tag_owns(tag, mid) as i64
}
