//! Tail id-switch slices of [`super::str_method`] — the pad / repeat /
//! concat / trim / html-wrapper family and the locale / normalize /
//! regex-lane family. Split out of the parent under the 500-line file
//! discipline (rotation 146); a child module reaches its parent's
//! private items, so the extern block, the method-id constants and the
//! shared helpers all stay private with no visibility churn.

use core::ffi::c_void;

use super::*;

/// Middle id-switch slice of [`str_method`] (200-line fn
/// discipline, same cascade shape as [`str_method_ext`] below) —
/// the pad / repeat / concat / trim / html-wrapper family.
/// Unmatched ids fall through to the locale/regex slice.
pub(super) unsafe fn str_method_pad(s: *mut u8, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_PAD_START || m == ANY_METHOD_PAD_END => {
                // §22.1.3.17 steps 2/4 — ?ToLength(maxLength) aborts
                // before ?ToString(fillString) runs (ReturnIfAbrupt,
                // 刀 21 posture).
                let target = to_index(arg_at(0), 0);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let end = (m == ANY_METHOD_PAD_END) as i64;
                let pad_av = arg_at(1);
                if is_undefined(pad_av) {
                    __torajs_str_any_pad(s, target, core::ptr::null(), end)
                } else {
                    let pad = __torajs_anyv_to_str(pad_av);
                    if __torajs_throw_check() != 0 {
                        __torajs_str_drop(pad);
                        return VALUE_UNDEFINED;
                    }
                    let out = __torajs_str_any_pad(s, target, pad as *const u8, end);
                    __torajs_str_drop(pad);
                    out
                }
            }
            m if m == ANY_METHOD_REPEAT => __torajs_str_any_repeat(s, to_index(arg_at(0), 0)),
            m if m == ANY_METHOD_CONCAT => {
                // Fold left per §22.1.3.5 — the accumulator starts
                // as a fresh owned copy of the receiver (the
                // substring glue's full-range form), each argument
                // ToStrings through an owned temp.
                let mut acc = __torajs_str_any_substring(s, 0, i64::MAX);
                for i in 0..argc {
                    let arg = __torajs_anyv_to_str(arg_at(i));
                    let next = __torajs_str_any_concat2(acc as *const u8, arg as *const u8);
                    __torajs_str_drop(acc as *mut c_void);
                    __torajs_str_drop(arg);
                    acc = next;
                }
                acc
            }
            m if m == ANY_METHOD_CODE_POINT_AT => {
                // OOB rides the -1 contract; spec answers undefined.
                let cp = __torajs_str_any_code_point_at(s, to_index(arg_at(0), 0));
                if cp < 0 {
                    VALUE_UNDEFINED
                } else {
                    __torajs_anyv_box_i64(cp)
                }
            }
            m if m == ANY_METHOD_LOCALE_COMPARE => {
                // ToString the comparand (a missing slot stringifies
                // to "undefined" per §7.1.19).
                let other = __torajs_anyv_to_str(arg_at(0));
                let ord = __torajs_str_any_locale_compare(s, other as *const u8);
                __torajs_str_drop(other);
                __torajs_anyv_box_i64(ord)
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
            _ => str_method_ext(s, mid, argv, argc),
        }
    }
}

/// Second id-switch slice of [`str_method`] (200-line fn
/// discipline, chunk 449 cascade shape) — the locale/normalize/
/// regex-lane family. Unmatched ids answer the shared
/// no-such-method TypeError.
unsafe fn str_method_ext(s: *mut u8, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_NORMALIZE => {
                let form_av = arg_at(0);
                if is_undefined(form_av) {
                    __torajs_str_any_normalize(s, core::ptr::null())
                } else {
                    let form = __torajs_anyv_to_str(form_av);
                    let out = __torajs_str_any_normalize(s, form as *const u8);
                    __torajs_str_drop(form);
                    out
                }
            }
            m if m == ANY_METHOD_TO_LOCALE_UPPER_CASE || m == ANY_METHOD_TO_LOCALE_LOWER_CASE => {
                // ES402 — the locale argument selects the tailored
                // SpecialCasing rule set (tr/az/lt); undefined =
                // host default; a string is validated by the kernel
                // (RangeError); a Tag::Arr walks
                // CanonicalizeLocaleList on the anyvalue side; any
                // other type is a length-0 array-like per
                // ToObject = host default.
                let up = (m == ANY_METHOD_TO_LOCALE_UPPER_CASE) as i64;
                let loc_av = arg_at(0);
                let cell_tag = if is_cell(loc_av) {
                    let p = as_void_ptr(loc_av);
                    (p.cast::<u8>().add(4) as *const u16).read()
                } else {
                    u16::MAX
                };
                if cell_tag == Tag::Arr as u16 {
                    __torajs_str_any_locale_case_arr(
                        s as *const u8,
                        as_void_ptr(loc_av) as *const c_void,
                        up,
                    )
                } else if cell_tag == Tag::Str as u16 || crate::nanbox::is_short_str(loc_av) {
                    let lc = __torajs_anyv_to_str(loc_av);
                    let out = __torajs_str_any_locale_case(s, lc as *const u8, up);
                    __torajs_str_drop(lc);
                    out
                } else {
                    __torajs_str_any_locale_case(s, core::ptr::null(), up)
                }
            }
            // ES2024 §22.1.3.10/33 — torajs Str is internally UTF-8
            // and well-formed by construction (the typed tier's
            // short-circuit wedge answers the same constants):
            // isWellFormed is always true, toWellFormed is identity
            // (a fresh owned full-range copy via the substring glue).
            m if m == ANY_METHOD_IS_WELL_FORMED => __torajs_anyv_box_from_pair(1, 1),
            m if m == ANY_METHOD_TO_WELL_FORMED => __torajs_str_any_substring(s, 0, i64::MAX),
            m if m == ANY_METHOD_LAST_INDEX_OF => {
                // §22.1.3.11 — a NaN fromIndex means +Infinity
                // (search the whole string), unlike indexOf's 0, so
                // this arm can't ride to_index's NaN→0. Steps 3-4:
                // each coercion aborts (ReturnIfAbrupt, 刀 21
                // posture — the indexOf twin's pair).
                let needle = __torajs_anyv_to_str(arg_at(0));
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(needle);
                    return VALUE_UNDEFINED;
                }
                let from_av = arg_at(1);
                let from = if is_undefined(from_av) {
                    i64::MAX
                } else {
                    let n = crate::nanbox_ffi::__torajs_anyv_to_number(from_av);
                    if __torajs_throw_check() != 0 {
                        __torajs_str_drop(needle);
                        return VALUE_UNDEFINED;
                    }
                    if n.is_nan() { i64::MAX } else { n as i64 }
                };
                let idx = __torajs_str_any_last_index_of(s, needle as *const u8, from);
                __torajs_str_drop(needle);
                __torajs_anyv_box_i64(idx)
            }
            m if m == ANY_METHOD_SEARCH => {
                // RFC 20260716 刀 8 — ES §22.1.3.13, same shape as
                // `match` (see above); the search kernel returns an
                // i64 index.
                let arg = arg_at(0);
                let Some(re_ptr) = coerce_regexp(arg, b"") else {
                    return VALUE_UNDEFINED;
                };
                let idx = __torajs_str_any_search(s, re_ptr);
                regexp_drop_if_coerced(arg, re_ptr);
                __torajs_anyv_box_i64(idx)
            }
            m if m == ANY_METHOD_MATCH_ALL => {
                // RFC 20260716 刀 8 — ES §22.1.3.12 step 4.c: a
                // non-RegExp arg coerces via `RegExpCreate(P, "g")`
                // (the `g` flag is mandatory here because the
                // matchAll kernel throws TypeError on non-global
                // regex; the coerced RegExp gets it implicitly).
                let arg = arg_at(0);
                let Some(re_ptr) = coerce_regexp(arg, b"g") else {
                    return VALUE_UNDEFINED;
                };
                let out = __torajs_str_any_match_all(s, re_ptr);
                regexp_drop_if_coerced(arg, re_ptr);
                out
            }
            _ => method_no_such(),
        }
    }
}
