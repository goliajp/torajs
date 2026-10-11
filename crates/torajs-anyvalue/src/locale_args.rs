//! The `locales` / `options` gate of the `toLocaleString` family
//! (Number, BigInt, Date and its date / time variants; Array and
//! TypedArray forward both arguments to their elements).
//!
//! The runtime ships no locale data: its only formatter is the en-US
//! one. ECMA-402 would format `(1234.5).toLocaleString("de-DE")` as
//! "1.234,5"; answering "1,234.5" instead is a wrong value, not a
//! fallback. So a request the formatter cannot serve is refused with a
//! RangeError, in the order the ECMA-402 constructors read their
//! arguments:
//!
//! - `locales` goes through CanonicalizeLocaleList (§9.2.1): undefined
//!   and an empty list are served; `null` is a TypeError (ToObject);
//!   a malformed tag is a RangeError; the first requested tag must be
//!   `en` or `en-US` (any case), else RangeError — raised where
//!   ResolveLocale would look it up, after the options are read.
//! - `options`: undefined is served, `null` is a TypeError
//!   (CoerceOptionsToObject). For an object, every option the
//!   constructor reads is read with [[Get]] in the constructor's order,
//!   and the first one that is not undefined is a RangeError. Keys the
//!   constructor never reads change nothing and are not looked at; a
//!   primitive coerces to a wrapper that carries none of these names.

use core::ffi::c_void;

use crate::locale_list::{first_valid_tag, is_object, is_string};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_null, is_undefined};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_str};

unsafe extern "C" {
    /// torajs-str — the served-locale gate (pending RangeError, 0).
    fn __torajs_str_locale_format_check(locale: *const u8) -> i64;
    fn __torajs_str_locale_refuse(locale: *const u8);
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
    fn __torajs_date_get_time(d: *const c_void) -> f64;
}

/// The options of Intl.NumberFormat (Number and BigInt).
pub(crate) const LOCALE_FORMAT_NUMBER: i64 = 0;
/// The options of Intl.DateTimeFormat (Date).
pub(crate) const LOCALE_FORMAT_DATE: i64 = 1;

/// InitializeNumberFormat's option reads, in order.
const NUMBER_OPTIONS: [&str; 21] = [
    "localeMatcher",
    "numberingSystem",
    "style",
    "currency",
    "currencyDisplay",
    "currencySign",
    "unit",
    "unitDisplay",
    "notation",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingIncrement",
    "roundingMode",
    "roundingPriority",
    "trailingZeroDisplay",
    "compactDisplay",
    "useGrouping",
    "signDisplay",
];

/// CreateDateTimeFormat's option reads, in order.
const DATE_OPTIONS: [&str; 20] = [
    "localeMatcher",
    "calendar",
    "numberingSystem",
    "hour12",
    "hourCycle",
    "timeZone",
    "weekday",
    "era",
    "year",
    "month",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "fractionalSecondDigits",
    "timeZoneName",
    "formatMatcher",
    "dateStyle",
    "timeStyle",
];

/// See the module doc. `kind` is [`LOCALE_FORMAT_NUMBER`] or
/// [`LOCALE_FORMAT_DATE`]. Leaves a pending throw on refusal; both
/// arguments are borrowed.
///
/// # Safety
/// Cell payloads of `locales` / `options` are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_locale_args_check(locales: u64, options: u64, kind: i64) {
    unsafe {
        match canonical_locales(locales) {
            Locales::Refused => {}
            Locales::Served => options_served(options, kind),
            // ResolveLocale looks the tag up after the options are read
            Locales::Unsupported(tag) => {
                options_served(options, kind);
                if __torajs_throw_check() == 0 {
                    __torajs_str_locale_refuse(tag);
                }
                __torajs_str_drop(tag as *mut c_void);
            }
        }
    }
}

/// What CanonicalizeLocaleList and the served-locale test made of
/// `locales`.
enum Locales {
    /// Nothing requested, or a tag the en-US formatter serves.
    Served,
    /// A pending throw (null, a malformed tag, a bad element).
    Refused,
    /// A valid first tag no locale data serves (an owned Str).
    Unsupported(*mut u8),
}

/// The Date face: §21.4.4.39 et al. answer "Invalid Date" for a NaN
/// time value before any locale or option is looked at.
///
/// # Safety
/// `d` is a live Date cell; as [`__torajs_locale_args_check`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_locale_args_check_date(
    d: *const c_void,
    locales: u64,
    options: u64,
) {
    unsafe {
        if !__torajs_date_get_time(d).is_nan() {
            __torajs_locale_args_check(locales, options, LOCALE_FORMAT_DATE);
        }
    }
}

/// The any-lane face over a call's argument vector: true when the
/// call may format (no arguments, or a served locale / options pair).
///
/// # Safety
/// `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn locale_argv_served(argv: *const u64, argc: i64, kind: i64) -> bool {
    if argc <= 0 {
        return true;
    }
    unsafe {
        let options = if argc >= 2 {
            *argv.add(1)
        } else {
            VALUE_UNDEFINED
        };
        __torajs_locale_args_check(*argv, options, kind);
        __torajs_throw_check() == 0
    }
}

/// CanonicalizeLocaleList, then the served-locale test on its first
/// entry.
unsafe fn canonical_locales(locales: AnyValue) -> Locales {
    unsafe {
        if is_undefined(locales) {
            return Locales::Served;
        }
        if is_null(locales) {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object".as_ptr());
            return Locales::Refused;
        }
        let first = if is_string(locales) {
            __torajs_anyv_to_str(locales) as *mut u8
        } else if is_object(locales) {
            let Ok(first) = first_valid_tag(as_void_ptr(locales)) else {
                return Locales::Refused;
            };
            first
        } else {
            // a number / boolean / symbol / bigint wrapper has no
            // elements: the empty list
            return Locales::Served;
        };
        if first.is_null() {
            return Locales::Served;
        }
        match __torajs_str_locale_format_check(first) {
            2 => Locales::Unsupported(first),
            served => {
                __torajs_str_drop(first as *mut c_void);
                if served == 1 {
                    Locales::Served
                } else {
                    Locales::Refused
                }
            }
        }
    }
}

unsafe fn options_served(options: AnyValue, kind: i64) {
    unsafe {
        if is_undefined(options) {
            return;
        }
        if is_null(options) {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object".as_ptr());
            return;
        }
        if !is_object(options) {
            return;
        }
        let names: &[&str] = if kind == LOCALE_FORMAT_DATE {
            &DATE_OPTIONS
        } else {
            &NUMBER_OPTIONS
        };
        for name in names {
            let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
            let v = crate::member_get_receiver::__torajs_any_member_get_with_receiver(
                options,
                key as *const c_void,
                options,
            );
            __torajs_str_drop(key as *mut c_void);
            let given = !is_undefined(v);
            __torajs_anyv_rc_dec(v);
            if __torajs_throw_check() != 0 {
                return;
            }
            if given {
                raise_unsupported_option(name);
                return;
            }
        }
    }
}

fn raise_unsupported_option(name: &str) {
    const PREFIX: &[u8] = b"unsupported option: ";
    const SUFFIX: &[u8] = b" (locale-sensitive formatting options are not available)";
    let mut buf = [0u8; 128];
    let mut n = 0;
    for part in [PREFIX, name.as_bytes(), SUFFIX] {
        buf[n..n + part.len()].copy_from_slice(part);
        n += part.len();
    }
    unsafe { __torajs_throw_range_error(buf.as_ptr() as *const core::ffi::c_char) };
}
