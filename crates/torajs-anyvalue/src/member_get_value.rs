//! Value channel of the any-lane member read — split from
//! `member_get.rs` under the 500-line file rule (the 刀 9 prototype
//! hook tipped it). `__torajs_any_member_get_value` mirrors the tag
//! channel arm-for-arm (see `member_get.rs`'s module doc for the
//! pair protocol); `reify_value` is the value twin of `reify_tag`.

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

use crate::member_get::{closure_props, is_wrapper_tag, recv_cell, wrapper_props};
use crate::member_get_own::{
    arr_own_pair, array_proto_props, closure_virtual_pair, dynobj_proto_pair, function_proto_props,
    strwrapper_length, user_proto_cell, wrapper_proto_props,
};
use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — own-entry existence (disambiguates a stored
    /// `undefined` from absent: `get_tag` answers 5 for both).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando twin of the dynobj_has probe.
    fn __torajs_arrprops_has(arr: *mut c_void, key: *const c_void) -> i32;
    /// torajs-arr — expando probe through the props slot.
    fn __torajs_arrprops_get_tag(arr: *mut c_void, key: *const c_void) -> u64;
    fn __torajs_arrprops_get_value(arr: *mut c_void, key: *const c_void) -> u64;
}

/// See `member_get.rs`'s module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_value(recv: AnyValue, key: *const c_void) -> u64 {
    match recv_cell(recv) {
        // Miss → builtin-proto own-method cell bits (0 = absent),
        // pairing the tag channel's fallthrough above. The nonzero
        // hit path stays a single hash probe — only a 0 slot (absent
        // OR a stored 0/false/null payload) pays the tag re-probe to
        // disambiguate.
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe {
            let v = __torajs_dynobj_get_value(ptr, key);
            if v == 0 && __torajs_dynobj_get_tag(ptr, key) == 5 {
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(ptr, key) != 0 {
                    return 0;
                }
                // Annex B §B.2.2.1 dynamic-key `__proto__` read —
                // see the tag twin.
                if crate::prop_has::key_is(key, b"__proto__") {
                    return dynobj_proto_pair(ptr).1;
                }
                // Knife 2 — user chain before builtin reify
                // (tag twin above).
                if let Some(parent) = user_proto_cell(ptr) {
                    return __torajs_any_member_get_value(parent, key);
                }
                let cell = crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key);
                if cell != 0 {
                    return cell;
                }
                // Inherited Object.prototype reify (tag twin above).
                return reify_value(recv, key);
            }
            v
        },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe {
            if let Some((_, val)) = arr_own_pair(ptr, key) {
                return val;
            }
            if __torajs_arrprops_get_tag(ptr, key) != 5 {
                return __torajs_arrprops_get_value(ptr, key);
            }
            // Stored-undefined shadow — see the tag twin.
            if __torajs_arrprops_has(ptr, key) != 0 {
                return 0;
            }
            // Inherited Array.prototype expando — tag twin above.
            let ap = array_proto_props();
            if !ap.is_null() {
                if __torajs_dynobj_get_tag(ap, key) != 5 {
                    return __torajs_dynobj_get_value(ap, key);
                }
                if __torajs_dynobj_has(ap, key) != 0 {
                    return 0;
                }
            }
            // Arr-cell singleton own surface — tag twin above.
            let cell = crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key);
            if cell != 0 {
                return cell;
            }
            reify_value(recv, key)
        },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe {
            let props = closure_props(ptr);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            if let Some((_, val)) = closure_virtual_pair(ptr, key) {
                return val;
            }
            // Plain-fn `.prototype` materialization — tag twin above.
            if crate::prop_has::key_is(key, b"prototype")
                && let Some((_, val)) = crate::closure_proto::fn_prototype_pair(ptr)
            {
                return val;
            }
            // Inherited Function.prototype expando — tag twin above.
            let fp = function_proto_props();
            if !fp.is_null() {
                if __torajs_dynobj_get_tag(fp, key) != 5 {
                    return __torajs_dynobj_get_value(fp, key);
                }
                if __torajs_dynobj_has(fp, key) != 0 {
                    return 0;
                }
            }
            reify_value(recv, key)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper own-
        // property expando value probe (mirror of the closure arm).
        Some((ptr, t)) if is_wrapper_tag(t) => unsafe {
            if t == Tag::StringWrapper as u16
                && let Some(len) = strwrapper_length(ptr, key)
            {
                return len;
            }
            let props = wrapper_props(ptr);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            // Inherited <Wrapper>.prototype expando — tag twin above.
            let wp = wrapper_proto_props(t);
            if !wp.is_null() {
                if __torajs_dynobj_get_tag(wp, key) != 5 {
                    return __torajs_dynobj_get_value(wp, key);
                }
                if __torajs_dynobj_has(wp, key) != 0 {
                    return 0;
                }
            }
            reify_value(recv, key)
        },
        // Chunk 744 — struct cell field probe (see the tag channel).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((_, val)) = crate::struct_probe::struct_field_pair(ptr, key) {
                // Absent error `message` — prototype-chain read
                // (mirror of the tag channel).
                if crate::struct_error_msg::error_message_absent_key(ptr, key) {
                    return crate::struct_error_msg::error_message_proto_pair(ptr).1;
                }
                return val;
            }
            // Blade 5 — a struct accessor has no AccessorPair cell to
            // hand over: the ZERO value channel is what tells
            // `__torajs_any_accessor_get` to take the struct lane.
            if crate::struct_probe::struct_accessor_key(ptr, key) {
                return 0;
            }
            // L3b ⑧ — class prototype chain (mirror of the tag
            // channel).
            let (ptag, pval) = crate::struct_error_msg::struct_proto_chain_pair(ptr, key);
            if ptag != AnySlotTag::Undef as u64 || pval != 0 {
                return pval;
            }
            reify_value(recv, key)
        },
        // §20.4.3.2 description — value channel (mirror of the tag
        // twin; NULL desc pairs with the Undef tag as absent-shaped
        // undefined).
        Some((ptr, t)) if t == Tag::Symbol as u16 => unsafe {
            if crate::prop_has::key_is(key, b"description") {
                return crate::member_get_layout::symbol_desc(ptr) as u64;
            }
            reify_value(recv, key)
        },
        _ => unsafe { reify_value(recv, key) },
    }
}

/// Value channel of `member_get.rs`'s `reify_tag` — the interned
/// cell's pointer bits (immortal, borrow-shaped like every other
/// probe answer).
///
/// # Safety
/// `key` is NULL or a live Str cell.
unsafe fn reify_value(recv: AnyValue, key: *const c_void) -> u64 {
    // L3b ④ — `.constructor` (mirror of the tag channel).
    if let Some(cell) = unsafe { crate::method_value::ctor_cell_for_recv(recv, key) } {
        return cell as u64;
    }
    // RFC 20260721 刀 3 — ctor-static / `prototype` / Number
    // constants (mirror of the tag channel).
    if let Some((ptr, t)) = recv_cell(recv)
        && t == Tag::Closure as u16
        && let Some((_, val)) = unsafe { crate::method_value::ctor_own_read_cell(ptr, key) }
    {
        return val;
    }
    unsafe { crate::method_value::builtin_method_lookup(recv, key) }
        .map(|c| c as u64)
        .unwrap_or(0)
}
