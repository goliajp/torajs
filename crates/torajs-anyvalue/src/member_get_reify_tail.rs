//! What the tag channel answers when nothing on the receiver claims
//! the key — split out of `member_get.rs`, which the
//! `Function.prototype` meta hop pushed past the 500-line limit.
//!
//! Every per-shape arm in the parent ends the same way: own entries,
//! then the shape's virtual faces, then here. This file is that tail
//! — the builtin-method reify a supported name resolves to, and the
//! DynObj arm's builtin-prototype leg in front of it.

use core::ffi::c_void;

use torajs_rc::Tag;

use torajs_rc::AnySlotTag;

use crate::member_get::{__torajs_any_member_get_tag, recv_cell};
use crate::nanbox::AnyValue;

/// Builtin-method reification probe (chunk 711) — a supported
/// method name on a builtin receiver answers a heap tag (the
/// interned function cell); everything else stays absent.
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn reify_tag(recv: AnyValue, key: *const c_void) -> u64 {
    // L3b ④ — `.constructor` answers the receiver family's interned
    // builtin-constructor cell (own-face shadows already probed).
    if unsafe { crate::method_value::ctor_cell_for_recv(recv, key) }.is_some() {
        return 4;
    }
    // RFC 20260721 刀 3 — a builtin ctor cell answers its table
    // statics / `prototype` / Number data constants as own reads
    // (borrow-shaped, matching the pair protocol).
    if let Some((ptr, t)) = recv_cell(recv)
        && t == Tag::Closure as u16
        && let Some((tag, _)) = unsafe { crate::method_value::ctor_own_read_cell(ptr, key) }
    {
        return tag;
    }
    if unsafe { crate::method_value::builtin_method_lookup(recv, key) }.is_some() {
        return 4;
    }
    // §10.1.8.1 step 4 — the walk does not end at the family
    // prototype (517-07), and it does not start at the root either
    // (525-04): the singletons in between are asked too.
    match unsafe { crate::member_get_proto_root::proto_chain_expando(recv, key) } {
        Some((t, _)) => t as u64,
        None => 5,
    }
}

/// DynObj-arm builtin tail (tag channel) — the own-method reify,
/// Function.prototype's virtual meta pair (§20.2.3, RFC 20260722
/// 刀 3), then the inherited Object.prototype reify (valueOf /
/// toLocaleString / the universal probes), same fallthrough as the
/// Arr / Closure / struct arms.
pub(crate) unsafe fn dynobj_builtin_tail_tag(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *const c_void,
) -> u64 {
    unsafe {
        if crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key) != 0 {
            return 4;
        }
        if let Some((mtag, _)) = crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key)
        {
            return mtag;
        }
        // A virtual accessor read directly off its proto singleton
        // runs the getter on the prototype itself — brand-check
        // throw (rationale on the helper).
        if crate::method_support_proto::proto_virtual_accessor_throws(ptr, key) {
            return AnySlotTag::Undef as u64;
        }
        // §10.1.8.1 step 4 — the implicit %Object.prototype% hop
        // (rationale on the helper).
        if let Some(parent) = crate::member_get_own::implicit_proto_parent(ptr) {
            return __torajs_any_member_get_tag(parent, key);
        }
        reify_tag(recv, key)
    }
}
