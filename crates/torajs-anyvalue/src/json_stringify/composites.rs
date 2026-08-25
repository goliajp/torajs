//! The two composite-object writers of [`super`] — the dynobj
//! entry walk and the struct-cell field walk. Split out of the
//! parent under the 500-line file discipline; a child module reaches
//! its parent's private items, so the extern block and the shared
//! helpers stay private with no visibility churn.

use core::ffi::c_void;

use super::*;

/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF` mirror (the u32 class tag
/// after the heap header of a `Tag::Obj` cell) — named so a class-tag
/// read-point sweep finds this walk.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// `{...}` — own enumerable entries in §10.1.11.1 order (the print
/// walker's `iter_order` contract); a key whose value serializes to
/// nothing is omitted entirely.
pub(super) unsafe fn write_object(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    unsafe {
        // §25.5.2.4 step 5 — an array replacer replaces the own-name
        // list wholesale, so that walk takes over entirely.
        if st.property_list.is_some() {
            return replacer::write_object_list(sb, box_void_ptr(ptr), depth, st);
        }
        __torajs_jsb_push_byte(sb, b'{');
        let len = __torajs_dynobj_iter_len(ptr);
        let order_layout = core::alloc::Layout::from_size_align(len as usize * 8, 8).unwrap();
        let order = if len > 0 {
            std::alloc::alloc_zeroed(order_layout) as *mut u64
        } else {
            core::ptr::null_mut()
        };
        let n = __torajs_dynobj_iter_order(ptr, order, len);
        let mut emitted = false;
        for j in 0..n {
            let i = *order.add(j as usize);
            if __torajs_dynobj_iter_flags(ptr, i) & BUCKET_FLAG_ENUMERABLE == 0 {
                continue;
            }
            let key = __torajs_dynobj_iter_key(ptr, i);
            if key.is_null() {
                continue;
            }
            // A replacer body runs mid-walk and may delete this very
            // entry, which frees the key cell the emit below still has
            // to quote — hold it across the call. The value is already
            // a snapshot either way, the same approximation
            // `json_reviver` documents for its own walk.
            let pinned = st.replacer.is_some();
            if pinned {
                crate::payload_rc_inc(4, key as i64);
            }
            emitted |= write_entry(sb, ptr, i, key, depth, st, emitted);
            if pinned {
                __torajs_str_drop(key);
            }
            if __torajs_throw_check() != 0 {
                break;
            }
        }
        if !order.is_null() {
            std::alloc::dealloc(order as *mut u8, order_layout);
        }
        if emitted {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}

/// One own enumerable entry of [`write_object`] — §25.5.2.2
/// SerializeJSONProperty for the dynobj lane. Answers whether it
/// contributed text (a key whose value serializes to nothing is
/// omitted entirely, so the caller's separator state must not
/// advance).
unsafe fn write_entry(
    sb: *mut c_void,
    ptr: *mut c_void,
    i: u64,
    key: *mut c_void,
    depth: u32,
    st: &St,
    emitted: bool,
) -> bool {
    unsafe {
        let mut value = __torajs_dynobj_iter_value(ptr, i);
        // §25.5.2.2 step 1 — the serialized value is ? Get(holder,
        // key): an accessor entry stores its AccessorPair cell, so
        // run the getter (receiver = the holder, borrowed into the
        // invoke) instead of serializing the pair as an empty
        // object. The result is OWNED (len_get's box_probe_pair
        // convention); a pending throw aborts the walk and
        // propagates through the caller's throw-check.
        let holder = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, ptr as i64);
        let mut owned = accessor_pair_of(value).is_some();
        if let Some(pair) = accessor_pair_of(value) {
            value = __torajs_accessor_invoke_getter(pair, holder);
            if __torajs_throw_check() != 0 {
                return false;
            }
        }
        // §25.5.2.3 step 2 — field-level toJSON hook.
        if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            value = r;
            owned = true;
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                return false;
            }
        }
        // §25.5.2.2 step 3 — the replacer answers the value actually
        // serialized, so it runs BEFORE the omit test below: an
        // `undefined` answer is exactly how a replacer drops a key.
        (value, owned) = replacer::apply(st, holder, key, value, owned);
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return false;
        }
        // Probe the value FIRST: an undefined / callable field
        // drops its key, so the separator and key bytes must not
        // be emitted speculatively. Serializing into a scratch
        // builder would cost an alloc per field — instead take
        // the cheap pre-check the split allows.
        if serializes_to_nothing(value) {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            return false;
        }
        if emitted {
            __torajs_jsb_push_byte(sb, b',');
        }
        push_indent(sb, depth + 1, st);
        __torajs_jsb_push_str_quoted(sb, key as *const u8);
        __torajs_jsb_push_byte(sb, b':');
        if !st.gap.is_empty() {
            __torajs_jsb_push_byte(sb, b' ');
        }
        write_value(sb, value, depth + 1, st);
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
        }
        true
    }
}

/// `{...}` — the Tag::Obj struct-cell twin of [`write_object`]. Reads
/// the instance's `class_tag` (`u32` at `+8`), looks the layout up in
/// the toolchain-emitted `__torajs_class_layouts` table, and walks
/// the fields in declaration order. Undefined / callable field values
/// drop their key (§25.5.2), same three-way split as dynobj. An
/// anonymous struct interned too late to receive a `class_tag`
/// (`class_tag == 0` → NULL layout) serializes as `{}` — matches the
/// same coverage gap `Object.keys(anonAny)` documents.
pub(super) unsafe fn write_struct(sb: *mut c_void, ptr: *mut c_void, depth: u32, st: &St) {
    unsafe {
        // Same §25.5.2.4 step 5 hand-off as the dynobj lane — the
        // list's `Get` reaches declared fields and expandos alike.
        if st.property_list.is_some() {
            return replacer::write_object_list(sb, box_void_ptr(ptr), depth, st);
        }
        __torajs_jsb_push_byte(sb, b'{');
        let class_tag = (ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF) as *const u32).read();
        let layout = __torajs_struct_layout_lookup(class_tag);
        let mut emitted = false;
        // §25.5.2 SerializeJSONObject walks EnumerableOwnProperties,
        // and an error instance's `message` / `stack` are `E:false`
        // (§20.5.6.1.1) — a layout field list cannot express that, so
        // the two names are filtered here. `struct_enum::error_prop_skip`
        // is the `Object.keys` twin of this test.
        let is_error = crate::member_get::header_flag(ptr, torajs_rc::FLAG_ERROR);
        if !layout.is_null() {
            let n = __torajs_struct_field_count(layout);
            for i in 0..n {
                let name = __torajs_struct_field_name(layout, i);
                if is_error {
                    let key = core::slice::from_raw_parts(name.ptr, name.len);
                    if key == b"message" || key == b"stack" {
                        continue;
                    }
                    // §20.5.3.2 — an unassigned `name` is not an own
                    // property at all (the class name is the
                    // prototype's), so it contributes no entry. An
                    // assigned one is ordinary and enumerable, and
                    // serializes like any other field.
                    if key == b"name" && crate::struct_error_msg::error_name_is_absent(ptr) {
                        continue;
                    }
                }
                // RFC 20260806-declared-field-redefine — the same
                // filter one line up, for an enumerability USER code
                // moved rather than the spec fixing. Free on any
                // instance never passed to defineProperty (the kernel
                // gates on a header bit).
                if __torajs_obj_field_is_nonenumerable(ptr, name.ptr, name.len as u32) != 0 {
                    continue;
                }
                let mut value: u64 = 0;
                if __torajs_struct_field_read_anyv(ptr, name.ptr, name.len as u32, &mut value) == 0
                {
                    continue;
                }
                // §25.5.2.3 step 2 — field-level toJSON hook (a
                // struct's Any field can hold a dynobj carrying one).
                let mut hook_owned = false;
                if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
                    value = r;
                    hook_owned = true;
                    if __torajs_throw_check() != 0 {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                        break;
                    }
                }
                // §25.5.2.2 step 3 — the field name lives in the
                // static layout table as raw bytes, so the call site
                // mints the Str the replacer's `key` argument needs.
                (value, hook_owned) = replacer::apply_named(
                    st,
                    crate::nanbox::box_void_ptr(ptr),
                    name.ptr,
                    name.len,
                    value,
                    hook_owned,
                );
                if __torajs_throw_check() != 0 {
                    if hook_owned {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    }
                    break;
                }
                if serializes_to_nothing(value) {
                    if hook_owned {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    }
                    continue;
                }
                if emitted {
                    __torajs_jsb_push_byte(sb, b',');
                }
                emitted = true;
                push_indent(sb, depth + 1, st);
                quote_bytes(sb, name.ptr, name.len);
                __torajs_jsb_push_byte(sb, b':');
                if !st.gap.is_empty() {
                    __torajs_jsb_push_byte(sb, b' ');
                }
                write_value(sb, value, depth + 1, st);
                if hook_owned {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                }
                if __torajs_throw_check() != 0 {
                    break;
                }
            }
        }
        // Blade 3 (RFC 20260714-struct-dynamic-props) — expando
        // entries after the layout fields (insertion order,
        // enumerable only; the pair read is a borrow). toJSON /
        // nothing handling mirrors the dynobj walk above; accessor
        // entries can't exist here yet (the defineProperty struct
        // arm is a recorded follow-up), so no getter dispatch.
        let props = crate::member_get_layout::struct_props(ptr)
            .cast_mut()
            .cast::<c_void>();
        if !props.is_null() && __torajs_throw_check() == 0 {
            let n_props = __torajs_dynobj_iter_len(props);
            let mut order = vec![0u64; n_props as usize];
            let n_ord = __torajs_dynobj_iter_order(props, order.as_mut_ptr(), n_props);
            for &pi in order.iter().take(n_ord as usize) {
                if __torajs_dynobj_iter_flags(props, pi) & BUCKET_FLAG_ENUMERABLE == 0 {
                    continue;
                }
                let key = __torajs_dynobj_iter_key(props, pi);
                if key.is_null() {
                    continue;
                }
                let mut value = __torajs_dynobj_iter_value(props, pi);
                let mut owned = false;
                if let Some(r) = crate::json_stringify_tojson::apply_tojson(value) {
                    value = r;
                    owned = true;
                    if __torajs_throw_check() != 0 {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                        break;
                    }
                }
                // §25.5.2.2 step 3 — the holder is the STRUCT cell,
                // not its expando props dynobj: `this` inside the
                // replacer is the object the program sees.
                (value, owned) =
                    replacer::apply(st, crate::nanbox::box_void_ptr(ptr), key, value, owned);
                if __torajs_throw_check() != 0 {
                    if owned {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    }
                    break;
                }
                if serializes_to_nothing(value) {
                    if owned {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    }
                    continue;
                }
                if emitted {
                    __torajs_jsb_push_byte(sb, b',');
                }
                emitted = true;
                push_indent(sb, depth + 1, st);
                __torajs_jsb_push_str_quoted(sb, key as *const u8);
                __torajs_jsb_push_byte(sb, b':');
                if !st.gap.is_empty() {
                    __torajs_jsb_push_byte(sb, b' ');
                }
                write_value(sb, value, depth + 1, st);
                if owned {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                }
                if __torajs_throw_check() != 0 {
                    break;
                }
            }
        }
        if emitted {
            push_indent(sb, depth, st);
        }
        __torajs_jsb_push_byte(sb, b'}');
    }
}
