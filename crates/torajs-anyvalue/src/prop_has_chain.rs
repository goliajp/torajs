//! §7.3.11 HasProperty on an `any` receiver — the OTHER membership
//! question, split out of `prop_has.rs` (file-size hard limit) so the
//! two are not read as one.
//!
//! `prop_has`'s `__torajs_any_prop_has` answers §7.3.10
//! HasOwnProperty: does the RECEIVER itself carry this key. This file
//! answers §7.3.11: does anything on the receiver's prototype CHAIN
//! carry it. `Object.hasOwn` and `in` are the two spellings, and the
//! own face is this one's first step.
//!
//! Consumers: the for-in mid-loop-delete guard (§14.7.5.9 re-checks
//! keys against the LIVE object each iteration, and an inherited key
//! is still present) and the `in` operator's kernel (torajs-rc
//! `in_op_any`, which rides this chain for the receiver shapes its
//! own cascade does not name).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::member_get::recv_cell;
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — own-entry presence (1 = live entry).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// §7.3.11 HasProperty — the own-property probe plus the user
/// [[Prototype]] chain walk (RFC 20260721 刀 5 R-F). No receiver
/// object-gate: the for-in mid-loop-delete guard consumes this on
/// snapshot receivers (§14.7.5.9 re-checks keys against the LIVE
/// object each iteration, and an inherited key is still present);
/// the `in` operator's rhs typecheck lives in its own kernel
/// (torajs-rc `in_op_any`, which also rides this chain).
///
/// # Safety
/// Same contract as [`crate::prop_has::__torajs_any_prop_has`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_has_property(recv: AnyValue, key: *const c_void) -> i64 {
    // §10.5.7 — a Proxy answers the whole question, chain included
    // (RFC 20260823-proxy-substrate 刀 2).
    if crate::proxy::is_proxy(recv) {
        return unsafe { crate::proxy_ops::has(crate::nanbox::as_void_ptr(recv), key) }
            .unwrap_or(false) as i64;
    }
    // §6.1.7 — a symbol key is a wholly separate key domain, and every
    // face past the own probe below is name-keyed (index decode,
    // interned method names, buffer names, ctor statics).
    // `member_get_symbol` owns the symbol chain for the READ face;
    // asking it here is what makes `in` and `[sym]` answer the same
    // walk. Before this hop the own probe answered and the arms below
    // answered 0 for every non-dynobj receiver, so `Symbol.iterator in
    // [1]` was false while `[1][Symbol.iterator]` was a function.
    if unsafe { crate::member_get_symbol::key_is_symbol(key) } {
        return unsafe { crate::member_get_symbol::symbol_key_has(recv, key) } as i64;
    }
    if unsafe { crate::prop_has::__torajs_any_prop_has(recv, key) } != 0 {
        return 1;
    }
    let Some((ptr, tag)) = recv_cell(recv) else {
        return 0;
    };
    // §7.3.11 step 2 — the buffer family's prototype half: accessor
    // names and interned methods (name-level, never invoking a
    // getter — an out-of-bounds DataView still answers true for
    // "byteLength" without throwing), then the %Object.prototype%
    // root like every other chain. The OWN half already answered
    // above, which is what keeps `Object.hasOwn` honest.
    if crate::member_get_buffer::is_buffer_family(tag) {
        if unsafe { crate::member_get_buffer::buffer_proto_key(tag, key) } {
            return 1;
        }
        let proto = unsafe {
            torajs_rc::builtin_proto::__torajs_get_builtin_prototype(
                torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64,
            )
        };
        return (!proto.is_null()
            && unsafe { __torajs_dynobj_has(proto as *const c_void, key) } != 0)
            as i64;
    }
    // §7.3.11 step 2 on a primitive-wrapper receiver — the own half
    // (inherent §22.1.4 face + expando) already answered above; the
    // chain half is the wrapper-prototype singleton's expando face
    // (`Boolean.prototype[1] = v`) and the %Object.prototype% root
    // behind it, which the singleton — a DynObj cell — walks itself.
    if let Some(ptag) = crate::member_get_layout::wrapper_proto_tag(tag) {
        let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(ptag) };
        if proto.is_null() {
            return 0;
        }
        return unsafe {
            __torajs_any_has_property(
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, proto as i64),
                key,
            )
        };
    }
    if tag != Tag::DynObj as u16 {
        return 0;
    }
    match unsafe { crate::member_get_own::user_proto_cell(ptr) } {
        Some(parent) => unsafe {
            __torajs_any_has_property(
                crate::nanbox_encode::__torajs_anyv_box_from_pair(4, parent as i64),
                key,
            )
        },
        None => {
            // Implicit chain root — the %Object.prototype% singleton's
            // expando face (digit keys installed via
            // `Object.prototype[0] = …`, RFC 20260721 G2d). A
            // null-proto receiver has no root; the singleton itself
            // must not re-probe (its own face already answered above).
            let flags = unsafe { ptr.cast::<u8>().add(6).cast::<u16>().read() };
            if flags & crate::member_get_own::DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
                return 0;
            }
            let proto = unsafe {
                torajs_rc::builtin_proto::__torajs_get_builtin_prototype(
                    torajs_rc::builtin_proto::OBJECT_PROTO_TAG as i64,
                )
            };
            if proto.is_null() || core::ptr::eq(proto.cast(), ptr) {
                return 0;
            }
            (unsafe { __torajs_dynobj_has(proto, key) } != 0) as i64
        }
    }
}
