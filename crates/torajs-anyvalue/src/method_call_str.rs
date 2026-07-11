//! `Tag::Str` arm of `__torajs_any_method_call` (Any-method-call
//! RFC 20260704 C2 + RC-2 match) — split out of `method_call.rs` by
//! the 500-line file discipline, mirroring the mapset / weak
//! siblings. Dispatches the interned method id onto the torajs-str
//! glue kernels; `match` routes a RegExp-cell argument through the
//! typed tier's match kernel (RFC 20260706-test262-bug-corpus RC-2).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_ANCHOR, ANY_METHOD_CHAR_AT, ANY_METHOD_ENDS_WITH, ANY_METHOD_INCLUDES,
    ANY_METHOD_INDEX_OF, ANY_METHOD_LINK, ANY_METHOD_MATCH, ANY_METHOD_REPLACE,
    ANY_METHOD_REPLACE_ALL, ANY_METHOD_SLICE, ANY_METHOD_SPLIT, ANY_METHOD_STARTS_WITH,
    ANY_METHOD_SUP, ANY_METHOD_TO_LOWER_CASE, ANY_METHOD_TO_UPPER_CASE, ANY_METHOD_TRIM,
    ANY_METHOD_TRIM_END, ANY_METHOD_TRIM_START, Tag,
};

use crate::method_call::{method_no_such, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_undefined};
use crate::nanbox_encode::{__torajs_anyv_box_from_pair, __torajs_anyv_box_i64};
use crate::nanbox_ffi::__torajs_anyv_to_str;

unsafe extern "C" {
    /// torajs-str — charAt glue (empty string for OOB).
    fn __torajs_str_any_char_at(s: *mut u8, idx: i64) -> u64;
    /// torajs-str — toUpperCase / toLowerCase glue.
    fn __torajs_str_any_case(s: *const u8, upper: i64) -> u64;
    /// torajs-str — indexOf glue (found code-unit index or -1).
    fn __torajs_str_any_index_of(s: *const u8, needle: *const u8, from: i64) -> i64;
    /// torajs-str — slice glue (missing end rides as i64::MAX).
    fn __torajs_str_any_slice(s: *const u8, start: i64, end: i64) -> u64;
    /// torajs-str — startsWith glue (1/0; kernel clamps pos).
    fn __torajs_str_any_starts_with(s: *const u8, needle: *const u8, pos: i64) -> i64;
    /// torajs-str — endsWith glue (1/0; missing end rides as i64::MAX).
    fn __torajs_str_any_ends_with(s: *const u8, needle: *const u8, end: i64) -> i64;
    /// torajs-str — split glue (NULL sep = no separator argument).
    fn __torajs_str_any_split(s: *const u8, sep: *const u8) -> u64;
    /// torajs-str — trim glue (mode: 0 both, 1 start, 2 end).
    fn __torajs_str_any_trim(s: *const u8, mode: i64) -> u64;
    /// torajs-str — match glue (owned_src materialize + heap-chain
    /// mark + boxed-null no-match).
    fn __torajs_str_any_match(s: *const u8, re: *const c_void) -> u64;
    /// torajs-str — replace/replaceAll glue, string-pattern lane.
    fn __torajs_str_any_replace(s: *const u8, needle: *const u8, repl: *const u8, all: i64) -> u64;
    /// torajs-str — replace/replaceAll glue, RegExp-pattern lane.
    fn __torajs_str_any_replace_regex(
        s: *const u8,
        re: *const c_void,
        repl: *const u8,
        all: i64,
    ) -> u64;
    /// torajs-str — annexB B.2.2 html-method glue (`mid` picks the
    /// CreateHTML form; NULL `val` rides as the JS `undefined`).
    fn __torajs_str_any_html(s: *const u8, mid: i64, val: *const u8) -> u64;
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
            m if m == ANY_METHOD_STARTS_WITH || m == ANY_METHOD_ENDS_WITH => {
                // ToString the needle (owned temp), test, drop.
                // startsWith defaults pos to 0; endsWith's missing
                // end rides as i64::MAX (kernel clamps to length).
                let needle = __torajs_anyv_to_str(arg_at(0));
                let hit = if m == ANY_METHOD_STARTS_WITH {
                    let pos = to_index(arg_at(1), 0);
                    __torajs_str_any_starts_with(s, needle as *const u8, pos)
                } else {
                    let end = to_index(arg_at(1), i64::MAX);
                    __torajs_str_any_ends_with(s, needle as *const u8, end)
                };
                __torajs_str_drop(needle);
                __torajs_anyv_box_from_pair(1, hit)
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
                // follow-up. The torajs-str glue materializes Substr
                // receivers and heap-chain-marks the product.
                let Some(re_ptr) = regexp_cell(arg_at(0)) else {
                    __torajs_throw_type_error(
                        c"s.match(...) on an any receiver requires a RegExp argument".as_ptr(),
                    );
                    return VALUE_UNDEFINED;
                };
                __torajs_str_any_match(s, re_ptr)
            }
            m if m == ANY_METHOD_REPLACE || m == ANY_METHOD_REPLACE_ALL => {
                // RC-2c — the pattern argument's cell tag picks the
                // lane; both replacement operands ToString through
                // owned temps this arm drops. A closure replacement
                // (the `_regex_fn` kernels) is a recorded follow-up.
                let all = (m == ANY_METHOD_REPLACE_ALL) as i64;
                let repl = __torajs_anyv_to_str(arg_at(1));
                let out = if let Some(re_ptr) = regexp_cell(arg_at(0)) {
                    __torajs_str_any_replace_regex(s, re_ptr, repl as *const u8, all)
                } else {
                    let needle = __torajs_anyv_to_str(arg_at(0));
                    let out =
                        __torajs_str_any_replace(s, needle as *const u8, repl as *const u8, all);
                    __torajs_str_drop(needle);
                    out
                };
                __torajs_str_drop(repl);
                out
            }
            m if m == ANY_METHOD_TRIM => __torajs_str_any_trim(s, 0),
            m if m == ANY_METHOD_TRIM_START => __torajs_str_any_trim(s, 1),
            m if m == ANY_METHOD_TRIM_END => __torajs_str_any_trim(s, 2),
            m if (ANY_METHOD_ANCHOR..=ANY_METHOD_SUP).contains(&m) => {
                // annexB B.2.2 html methods — the four attributed
                // forms (anchor/fontcolor/fontsize/link, ids 95-98)
                // ToString their value argument through an owned
                // temp; undefined rides as NULL (the kernel renders
                // the "undefined" text). Plain wraps ignore
                // arguments per CreateHTML.
                let val_av = arg_at(0);
                let is_attr = (ANY_METHOD_ANCHOR..=ANY_METHOD_LINK).contains(&m);
                if is_attr && !is_undefined(val_av) {
                    let v = __torajs_anyv_to_str(val_av);
                    let out = __torajs_str_any_html(s, m, v as *const u8);
                    __torajs_str_drop(v);
                    out
                } else {
                    __torajs_str_any_html(s, m, core::ptr::null())
                }
            }
            _ => method_no_such(),
        }
    }
}

/// The argument's RegExp cell pointer, or `None` for any non-RegExp
/// value (primitives, other cell tags).
unsafe fn regexp_cell(av: AnyValue) -> Option<*const c_void> {
    if !is_cell(av) {
        return None;
    }
    let p = as_void_ptr(av);
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    if tag == Tag::RegExp as u16 {
        Some(p as *const c_void)
    } else {
        None
    }
}
