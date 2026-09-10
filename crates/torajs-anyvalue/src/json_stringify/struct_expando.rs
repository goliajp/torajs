//! The struct arm's expando entries (RFC 20260714-struct-dynamic-props
//! blade 3), carved out of `composites::write_entry` when 628-05
//! made them a three-lane walk: a runtime-computed instance field
//! rides its DECLARATION row (torajs-meta `classmeta::computed_fields`
//! keeps the anchor; `__torajs_struct_expando_order` answers which
//! dict entries go before row `i`, after the last row, and the rest),
//! so `{"foo":1,"b":42,"bar":2}` comes out in definition order like
//! `Object.keys` does.
//!
//! toJSON / nothing handling mirrors the dynobj walk; accessor
//! entries can't exist here yet (the defineProperty struct arm is a
//! recorded follow-up), so no getter dispatch.

use core::ffi::c_void;

use super::*;

unsafe extern "C" {
    /// torajs-meta — the lane's dict iteration indices.
    fn __torajs_struct_expando_order(
        cell: *const c_void,
        lane: i64,
        out: *mut u64,
        cap: u64,
    ) -> u64;
}

/// `lane` of [`write_struct_expandos`] — the entries before row `i`.
pub(super) fn before_row(i: u32) -> i64 {
    i64::from(i)
}
/// `lane` — the entries after the last row.
pub(super) const TAIL: i64 = -2;
/// `lane` — every other expando, insertion order.
pub(super) const REST: i64 = -1;

/// Serialize the enumerable expando entries `lane` selects, as
/// `"key":value` members after whatever `emitted` says came before.
/// A pending throw (a hook's, a replacer's) ends the walk; the caller
/// checks it.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `sb` a live string builder.
pub(super) unsafe fn write_struct_expandos(
    sb: *mut c_void,
    ptr: *mut c_void,
    depth: u32,
    st: &St,
    emitted: &mut bool,
    lane: i64,
) {
    unsafe {
        let props = crate::member_get_layout::struct_props(ptr)
            .cast_mut()
            .cast::<c_void>();
        if props.is_null() || __torajs_throw_check() != 0 {
            return;
        }
        let n_props = __torajs_dynobj_iter_len(props);
        let mut order = vec![0u64; n_props as usize];
        let n_ord = __torajs_struct_expando_order(ptr, lane, order.as_mut_ptr(), n_props);
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
                    return;
                }
            }
            // §25.5.2.2 step 3 — the holder is the STRUCT cell, not
            // its expando props dynobj: `this` inside the replacer is
            // the object the program sees.
            (value, owned) =
                replacer::apply(st, crate::nanbox::box_void_ptr(ptr), key, value, owned);
            if __torajs_throw_check() != 0 {
                if owned {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                }
                return;
            }
            if serializes_to_nothing(value) {
                if owned {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                }
                continue;
            }
            if *emitted {
                __torajs_jsb_push_byte(sb, b',');
            }
            *emitted = true;
            push_indent(sb, depth + 1, st);
            __torajs_jsb_push_str_quoted(sb, key as *const u8);
            __torajs_jsb_push_byte(sb, b':');
            if st.has_gap() {
                __torajs_jsb_push_byte(sb, b' ');
            }
            write_value(sb, value, depth + 1, st);
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            }
            if __torajs_throw_check() != 0 {
                return;
            }
        }
    }
}
