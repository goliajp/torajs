//! §22.2.6.4 `get RegExp.prototype.flags` — the body behind the
//! reified accessor getter cell (`torajs_rc::ANY_METHOD_GET_FLAGS`,
//! `method_value::accessor_getter`). Receiver-generic by spec: a
//! non-object `this` is a TypeError, and for an object the eight flag
//! properties are read with `Get` in the order d g i m s u v y, each
//! ToBoolean'd, each getter's throw ending the walk — which is what
//! the test262 `flags/coercion-*` / `get-order` / `rethrow` family
//! observes through a plain object with getters. A real RegExp
//! receiver reads its own flag word through the same member face; the
//! instance's direct `re.flags` read (torajs-regex
//! `__torajs_regex_get_flags`) never comes here.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_cell};

unsafe extern "C" {
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv: AnyValue) -> AnyValue;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// dynobj accessor-pair bucket tag (torajs-dynobj layout mirror).
const ANY_ACCESSOR_TAG: u64 = 6;

/// The flag properties in §22.2.6.4 step order, each with the code
/// unit it contributes.
const FLAG_PROPS: [(&[u8], u8); 8] = [
    (b"hasIndices", b'd'),
    (b"global", b'g'),
    (b"ignoreCase", b'i'),
    (b"multiline", b'm'),
    (b"dotAll", b's'),
    (b"unicode", b'u'),
    (b"unicodeSets", b'v'),
    (b"sticky", b'y'),
];

/// # Safety
/// `recv` is a live any value.
pub(crate) unsafe fn regexp_flags_getter(recv: AnyValue) -> AnyValue {
    unsafe {
        if !crate::to_primitive::is_object_value(recv) {
            __torajs_throw_type_error(
                c"RegExp.prototype.flags getter called on a non-object".as_ptr(),
            );
            return VALUE_UNDEFINED;
        }
        let mut out = [0u8; 8];
        let mut n = 0usize;
        for (name, ch) in FLAG_PROPS {
            let key = crate::__torajs_str_alloc_pooled(name.len() as u64);
            core::ptr::copy_nonoverlapping(name.as_ptr(), key.add(16), name.len());
            let tag = crate::member_get::__torajs_any_member_get_tag(recv, key as *const c_void);
            let val =
                crate::member_get_value::__torajs_any_member_get_value(recv, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            // An accessor entry's getter runs with the receiver as
            // `this` and answers OWNED; a data entry is a borrow.
            let (v, owned) = if tag == ANY_ACCESSOR_TAG {
                let got = __torajs_accessor_invoke_getter(val as *const c_void, recv);
                if __torajs_throw_check() != 0 {
                    if is_cell(got) {
                        crate::nanbox_ffi::__torajs_anyv_rc_dec(got);
                    }
                    return VALUE_UNDEFINED;
                }
                (got, true)
            } else {
                (
                    crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, val as i64),
                    false,
                )
            };
            if crate::nanbox_ffi::__torajs_anyv_to_bool(v) {
                out[n] = ch;
                n += 1;
            }
            if owned && is_cell(v) {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            }
        }
        let s = crate::__torajs_str_alloc_pooled(n as u64);
        core::ptr::copy_nonoverlapping(out.as_ptr(), s.add(16), n);
        crate::nanbox_encode::__torajs_anyv_box_pointer(s as *mut c_void)
    }
}
