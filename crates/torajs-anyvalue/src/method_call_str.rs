//! `Tag::Str` arm of `__torajs_any_method_call` (Any-method-call
//! RFC 20260704 C2 + RC-2 match) — split out of `method_call.rs` by
//! the 500-line file discipline, mirroring the mapset / weak
//! siblings. Dispatches the interned method id onto the torajs-str
//! glue kernels; `match` routes a RegExp-cell argument through the
//! typed tier's match kernel (RFC 20260706-test262-bug-corpus RC-2).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_CHAR_AT, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_MATCH,
    ANY_METHOD_SLICE, ANY_METHOD_SPLIT, ANY_METHOD_TO_LOWER_CASE, ANY_METHOD_TO_UPPER_CASE,
    ANY_METHOD_TRIM, ANY_METHOD_TRIM_END, ANY_METHOD_TRIM_START, Tag,
};

use crate::method_call::{method_not_a_function, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
};
use crate::nanbox_ffi::__torajs_anyv_to_str;

unsafe extern "C" {
    /// torajs-str — charAt glue (empty string for OOB).
    fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64;
    /// torajs-regex — the typed tier's `s.match(re)` kernel: fresh
    /// +1 match Arr, or null for no match (spec §22.2.7.5/.8).
    fn __torajs_str_match_regex(s: *const c_void, re: *const c_void) -> *mut c_void;
    /// torajs-arr — elem-kind marker; 4 = KIND_HEAP_CHAIN (Str-cell
    /// slots) so any-tier index reads box the raw pointers on read
    /// (same treatment as `__torajs_str_any_split`'s product).
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    /// torajs-str — toUpperCase / toLowerCase glue.
    fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64;
    /// torajs-str — indexOf glue (found code-unit index or -1).
    fn __torajs_str_any_index_of(s: *const u8, needle: *const u8, from: i64) -> i64;
    /// torajs-str — slice glue (missing end rides as i64::MAX).
    fn __torajs_str_any_slice(s: *const u8, start: i64, end: i64) -> u64;
    /// torajs-str — split glue (NULL sep = no separator argument).
    fn __torajs_str_any_split(s: *const u8, sep: *const u8) -> u64;
    /// torajs-str — trim glue (mode: 0 both, 1 start, 2 end).
    fn __torajs_str_any_trim(s: *const u8, mode: i64) -> u64;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `Tag::Str` arm — id-switch onto the torajs-str glue.
pub(crate) unsafe fn str_method(s: *mut u8, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_CHAR_AT => __torajs_str_any_char_at(s, to_index(arg_at(0), 0)),
            m if m == ANY_METHOD_TO_UPPER_CASE => __torajs_str_any_case(s, 1),
            m if m == ANY_METHOD_TO_LOWER_CASE => __torajs_str_any_case(s, 0),
            m if m == ANY_METHOD_INDEX_OF || m == ANY_METHOD_INCLUDES => {
                // ToString the needle (owned temp), scan, drop.
                let needle = __torajs_anyv_to_str(arg_at(0));
                let from = to_index(arg_at(1), 0);
                let idx = __torajs_str_any_index_of(s, needle as *const u8, from);
                __torajs_str_drop(needle);
                if m == ANY_METHOD_INDEX_OF {
                    __torajs_anyv_box_i64(idx)
                } else {
                    __torajs_anyv_box_from_pair(1, (idx >= 0) as i64)
                }
            }
            m if m == ANY_METHOD_SLICE => {
                let start = to_index(arg_at(0), 0);
                let end = to_index(arg_at(1), i64::MAX);
                __torajs_str_any_slice(s, start, end)
            }
            m if m == ANY_METHOD_SPLIT => {
                let sep_av = arg_at(0);
                if is_undefined(sep_av) {
                    __torajs_str_any_split(s, core::ptr::null())
                } else {
                    let sep = __torajs_anyv_to_str(sep_av);
                    let out = __torajs_str_any_split(s, sep as *const u8);
                    __torajs_str_drop(sep);
                    out
                }
            }
            m if m == ANY_METHOD_MATCH => {
                // RC-2 (RFC 20260706-test262-bug-corpus) — RegExp-cell
                // argument only; the string→RegExp coercion lane is a
                // follow-up. Kernel ownership (+1 Arr / null) transfers
                // into the box; box_pointer maps null → VALUE_NULL.
                let re_av = arg_at(0);
                let mut re_ptr: *const c_void = core::ptr::null();
                if is_cell(re_av) {
                    let rp = as_void_ptr(re_av);
                    let rtag = (rp.cast::<u8>().add(4) as *const u16).read();
                    if rtag == Tag::RegExp as u16 {
                        re_ptr = rp;
                    }
                }
                if re_ptr.is_null() {
                    __torajs_throw_type_error(
                        c"s.match(...) on an any receiver requires a RegExp argument".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                }
                let out = __torajs_str_match_regex(s as *const c_void, re_ptr);
                if !out.is_null() {
                    __torajs_arr_mark_kind(out, 4); // KIND_HEAP_CHAIN
                }
                __torajs_anyv_box_pointer(out)
            }
            m if m == ANY_METHOD_TRIM => __torajs_str_any_trim(s, 0),
            m if m == ANY_METHOD_TRIM_START => __torajs_str_any_trim(s, 1),
            m if m == ANY_METHOD_TRIM_END => __torajs_str_any_trim(s, 2),
            _ => method_not_a_function(),
        }
    }
}
