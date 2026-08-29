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
use crate::nanbox::{AnyValue, is_short_str};

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
    /// torajs-regex — boxed-form lastIndex peek (BORROW; 0 = numeric
    /// form) + the numeric f64 getter.
    fn __torajs_regex_last_index_raw(re: *const c_void) -> u64;
    fn __torajs_regex_get_last_index(re: *const c_void) -> f64;
}

/// See `member_get.rs`'s module doc.
///
/// # Safety
/// Cell receivers are valid heap pointers; `key` is a live Str or
/// Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_get_value(recv: AnyValue, key: *const c_void) -> u64 {
    // Tag-channel twin of the Proxy arm — zero routes the sentinel
    // into the receiver-aware kernel (RFC 20260823-proxy-substrate).
    if crate::proxy::is_proxy(recv) {
        return 0;
    }
    // §6.1.7 — a symbol key takes its own short walk (own dict, then
    // what the receiver inherits); the cascade below is string-keyed.
    if unsafe { crate::member_get_symbol::key_is_symbol(key) } {
        return unsafe { crate::member_get_symbol::symbol_key_pair(recv, key) }.1;
    }
    // §10.4.3 ShortStr own face — tag twin in `member_get.rs`.
    if is_short_str(recv)
        && let Some((_, val)) = unsafe { crate::member_get_str::str_own_pair(recv, key) }
    {
        return val;
    }
    match recv_cell(recv) {
        // Miss → builtin-proto own-method cell bits (0 = absent),
        // pairing the tag channel's fallthrough above. The nonzero
        // hit path stays a single hash probe — only a 0 slot (absent
        // OR a stored 0/false/null payload) pays the tag re-probe to
        // disambiguate.
        Some((ptr, t)) if t == Tag::DynObj as u16 => unsafe { dynobj_arm_value(ptr, recv, key) },
        Some((ptr, t)) if t == Tag::Arr as u16 => unsafe { arr_arm_value(ptr, recv, key) },
        Some((ptr, t)) if t == Tag::Closure as u16 => unsafe { closure_arm_value(ptr, recv, key) },
        // Promise-cell own-property value probe via the +32 lazy
        // expando — tag twin in member_get.rs (rotation 353,
        // plan-state L3b ①).
        Some((ptr, t)) if t == Tag::Promise as u16 => unsafe {
            let props = crate::member_get_layout::promise_props(ptr);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            reify_value(recv, key)
        },
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — wrapper own-
        // property expando value probe (mirror of the closure arm).
        Some((ptr, t)) if is_wrapper_tag(t) => unsafe {
            if t == Tag::StringWrapper as u16 {
                if let Some(len) = strwrapper_length(ptr, key) {
                    return len;
                }
                // Inherent index face ahead of the expando — tag twin.
                if let Some(inner) = crate::wrapper_view_through::resolve_inner_recv(ptr, t)
                    && let Some((_, val)) = crate::member_get_str::str_own_pair(inner, key)
                {
                    return val;
                }
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
            // §20.4.3.2 description over a SymbolWrapper — value
            // channel twin of the tag arm (inner [[Description]]).
            if t == Tag::SymbolWrapper as u16 && crate::prop_has::key_is(key, b"description") {
                let inner = (ptr.cast::<u8>().add(8) as *const *const c_void).read();
                return crate::member_get_layout::symbol_desc(inner) as u64;
            }
            reify_value(recv, key)
        },
        // Chunk 744 — struct cell field probe (see the tag channel).
        Some((ptr, t)) if t == Tag::Obj as u16 => unsafe {
            if let Some((_, val)) = crate::struct_probe::struct_field_pair(ptr, key) {
                // Absent error `message` / `name` — prototype-chain
                // read (mirror of the tag channel).
                if crate::struct_error_msg::error_absent_key(ptr, key) {
                    return crate::struct_error_msg::struct_proto_chain_pair(ptr, key).1;
                }
                return val;
            }
            // Blade 5 — a struct accessor has no AccessorPair cell to
            // hand over: the ZERO value channel is what tells
            // `__torajs_any_accessor_get` to take the struct lane.
            if crate::struct_probe::struct_accessor_key(ptr, key) {
                return 0;
            }
            // Blade 2 — expando dict own face (mirror of the tag
            // channel; borrow-shaped like the dynobj bucket).
            let props = crate::member_get_layout::struct_props(ptr);
            if !props.is_null() && __torajs_dynobj_has(props, key) != 0 {
                return __torajs_dynobj_get_value(props, key);
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
        // §22.2.4.1 lastIndex — value channel (mirror of the tag
        // twin in member_get.rs; borrow-shaped like the dynobj
        // bucket).
        // §25.1.6 / §23.2.3 — buffer family, value twin of the tag
        // arm: expando bag first, then the prototype accessors, then
        // the builtin-method reify.
        Some((ptr, t)) if crate::member_get_buffer::is_buffer_family(t) => unsafe {
            let props = crate::member_get_layout::buffer_props(ptr, t);
            if !props.is_null() {
                if __torajs_dynobj_get_tag(props, key) != 5 {
                    return __torajs_dynobj_get_value(props, key);
                }
                // Stored-undefined shadow — see the tag twin.
                if __torajs_dynobj_has(props, key) != 0 {
                    return 0;
                }
            }
            match crate::member_get_buffer::buffer_family_prop(recv, key, t) {
                Some((_, val)) => val,
                None => reify_value(recv, key),
            }
        },
        Some((ptr, t)) if t == Tag::RegExp as u16 => unsafe { regexp_arm_value(ptr, key, recv) },
        // Map / Set / Date own bag — value twin of the tag arm.
        Some((ptr, t)) if crate::member_get_layout::is_stateful_bag_tag(t) => unsafe {
            match expando_arm_value(ptr, t, key) {
                Some(v) => v,
                None => reify_value(recv, key),
            }
        },
        // §10.4.3 heap Str / Substr own face — tag twin.
        Some((_, t)) if t == Tag::Str as u16 => unsafe {
            if let Some((_, val)) = crate::member_get_str::str_own_pair(recv, key) {
                return val;
            }
            reify_value(recv, key)
        },
        _ => unsafe { reify_value(recv, key) },
    }
}

/// `Tag::Closure` value channel — arm-for-arm twin of
/// `member_get.rs`'s `closure_arm_tag`, extracted alongside it under
/// the 200-line function rule (the 405-01 user-[[Prototype]] chain
/// hop tipped both).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell; `key` is a live Str cell;
/// `recv` NaN-boxes the closure.
unsafe fn closure_arm_value(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
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
        // Plain-fn `.prototype` materialization — tag twin.
        if crate::prop_has::key_is(key, b"prototype")
            && let Some((_, val)) = crate::closure_proto::fn_prototype_pair(ptr)
        {
            return val;
        }
        // 405-01 substrate — user [[Prototype]] chain, tag twin.
        match crate::member_get_own::closure_user_proto(ptr) {
            Some(Some(parent)) => return __torajs_any_member_get_value(parent, key),
            Some(None) => return 0,
            None => {}
        }
        // Inherited Function.prototype expando — tag twin.
        // §20.2.3 makes `Function.prototype` a built-in FUNCTION
        // object, so its own virtual `length` / `name` pair is
        // reached through this arm rather than the dynobj one. The
        // probe answers None for every ordinary closure.
        if let Some((_, mval)) = crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key)
        {
            return mval;
        }
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
    }
}

/// `Tag::Arr` value channel — arm-for-arm twin of `member_get.rs`'s
/// `arr_arm_tag`, extracted alongside it under the 200-line function
/// rule.
///
/// # Safety
/// `ptr` is a live `Tag::Arr` cell; `key` is a live Str cell; `recv`
/// NaN-boxes the array.
unsafe fn arr_arm_value(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
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
    }
}

/// The `Tag::DynObj` arm — value channel of the tag twin's dynobj
/// cascade (own probe → stored-undefined shadow → globalThis miss →
/// builtin-proto own faces → the reify tail), lifted out when the
/// direct proto-accessor gate pushed the dispatcher past the 200
/// hard line (r483).
unsafe fn dynobj_arm_value(ptr: *mut c_void, recv: AnyValue, key: *const c_void) -> u64 {
    unsafe {
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
            // Explicit null proto cuts the chain — tag twin
            // above (§10.1.8.1 OrdinaryGet step 2).
            if crate::member_get_own::dynobj_null_proto(ptr) {
                return 0;
            }
            // G2 globalThis missing-known probe — tag twin
            // above (the tag channel already threw; answering 0
            // here keeps the pair protocol coherent).
            if crate::method_value::globalthis_object::globalthis_missing_known(ptr, key) {
                return 0;
            }
            let cell = crate::method_support::__torajs_builtin_proto_own_method_cell(ptr, key);
            if cell != 0 {
                return cell;
            }
            // Function.prototype's virtual own name/length pair
            // (§20.2.3, RFC 20260722 刀 3) — tag twin above.
            if let Some((_, mval)) =
                crate::method_support_proto_meta::builtin_proto_own_meta(ptr, key)
            {
                return mval;
            }
            // Direct proto-accessor read (tag twin above) — the
            // getter brand-checks its own prototype and throws.
            if crate::method_support_proto::proto_virtual_accessor_throws(ptr, key) {
                return 0;
            }
            // Implicit %Object.prototype% hop — tag twin above.
            if let Some(parent) = crate::member_get_own::implicit_proto_parent(ptr) {
                return __torajs_any_member_get_value(parent, key);
            }
            // Inherited Object.prototype reify (tag twin above).
            return reify_value(recv, key);
        }
        v
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
    if let Some(c) = unsafe { crate::method_value::builtin_method_lookup(recv, key) } {
        return c as u64;
    }
    // Mirror of the tag channel (517-07).
    unsafe { crate::member_get_proto_root::object_proto_expando_value(key) }
}

/// Own-bag probe on the value channel — twin of
/// `member_get::expando_arm_tag`, and the two must agree key for key
/// (a stored `undefined` claims the key on both).
///
/// # Safety
/// `ptr` is a live cell whose header tag is `cell_tag`; `key` is
/// NULL or a live Str cell.
unsafe fn expando_arm_value(ptr: *mut c_void, cell_tag: u16, key: *const c_void) -> Option<u64> {
    let props = unsafe { crate::member_get_layout::expando_props(ptr, cell_tag) };
    if props.is_null() {
        return None;
    }
    unsafe {
        if __torajs_dynobj_get_tag(props, key) != 5 || __torajs_dynobj_has(props, key) != 0 {
            return Some(__torajs_dynobj_get_value(props, key));
        }
    }
    None
}

/// §22.2.4.1 `lastIndex` on the value channel — the twin of
/// `member_get::regexp_arm_tag`, lifted out for the same reason.
///
/// # Safety
/// `ptr` is a live RegExp cell; `key` is NULL or a live Str cell;
/// `recv` boxes `ptr`.
unsafe fn regexp_arm_value(ptr: *mut c_void, key: *const c_void, recv: AnyValue) -> u64 {
    unsafe {
        if crate::prop_has::key_is(key, b"lastIndex") {
            let raw = __torajs_regex_last_index_raw(ptr);
            if raw != 0 {
                return crate::__torajs_anyv_unbox_value(raw) as u64;
            }
            return __torajs_regex_get_last_index(ptr).to_bits();
        }
        if let Some(v) = expando_arm_value(ptr, Tag::RegExp as u16, key) {
            return v;
        }
        reify_value(recv, key)
    }
}
