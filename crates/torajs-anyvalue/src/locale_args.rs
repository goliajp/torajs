//! The `locales` / `options` arguments of the `toLocaleString` family
//! (Number, BigInt, Date and its date / time variants), handled the way
//! the ECMA-402 constructors they stand for handle them. Array and
//! TypedArray never come here: they invoke each element's
//! `toLocaleString` with no arguments (§23.1.3.32), so their own
//! `locales` / `options` are not read.
//!
//! - `locales` goes through CanonicalizeLocaleList (§9.2.1): undefined
//!   is the empty list, `null` a TypeError (ToObject), an element that
//!   is neither a String nor an Object a TypeError, a structurally
//!   invalid tag (§6.2.1) a RangeError. Every valid tag is accepted.
//!   The runtime's only locale data is en-US, which is also the default
//!   locale, so LookupMatcher (§9.2.2 / §9.2.4) resolves any list to
//!   en-US: a tag that canonicalizes to it (`EN-us`, `en-Latn-US`,
//!   `en-US-u-nu-latn`) is served as itself, any other falls back.
//! - `options` goes through CoerceOptionsToObject, then every option the
//!   constructor reads is read in its order, with its range checks
//!   ([`crate::locale_options_number`], [`crate::locale_options_date`]).
//!   An option the formatter does not apply yet is read and ignored.

use core::ffi::c_void;

use crate::locale_list::{first_valid_tag, is_object, is_string};
use crate::locale_options::{AnyOptions, Read, Refusal, raise};
use crate::locale_options_date::{Required, read_date_time_format_options};
use crate::locale_options_number::read_number_format_options;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_null, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_to_str;
use torajs_rc::{
    ANY_METHOD_TO_LOCALE_DATE_STRING, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_LOCALE_TIME_STRING,
};

unsafe extern "C" {
    fn __torajs_str_locale_check(locale: *const u8) -> i64;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_date_get_time(d: *const c_void) -> f64;
    fn __torajs_date_to_locale_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_date_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_time_string(d: *const c_void) -> *mut u8;
}

/// The options of Intl.NumberFormat (Number and BigInt).
pub(crate) const LOCALE_FORMAT_NUMBER: i64 = 0;
/// The options of Intl.DateTimeFormat for `toLocaleString`.
pub(crate) const LOCALE_FORMAT_DATE_ANY: i64 = 1;
/// ... for `toLocaleDateString`.
pub(crate) const LOCALE_FORMAT_DATE_DATE: i64 = 2;
/// ... for `toLocaleTimeString`.
pub(crate) const LOCALE_FORMAT_DATE_TIME: i64 = 3;

/// See the module doc. `kind` is one of the `LOCALE_FORMAT_*`
/// constants. Leaves a pending throw on refusal; both arguments are
/// borrowed.
///
/// # Safety
/// Cell payloads of `locales` / `options` are live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_locale_args_check(locales: u64, options: u64, kind: i64) {
    let checked = unsafe { canonical_locales(locales) }.and_then(|()| {
        let mut opts = AnyOptions::coerce(options)?;
        match kind {
            LOCALE_FORMAT_NUMBER => read_number_format_options(&mut opts),
            LOCALE_FORMAT_DATE_DATE => read_date_time_format_options(&mut opts, Required::Date),
            LOCALE_FORMAT_DATE_TIME => read_date_time_format_options(&mut opts, Required::Time),
            _ => read_date_time_format_options(&mut opts, Required::Any),
        }
    });
    if let Err(r) = checked {
        raise(r);
    }
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
    kind: i64,
) {
    unsafe {
        if !__torajs_date_get_time(d).is_nan() {
            __torajs_locale_args_check(locales, options, kind);
        }
    }
}

/// The any-lane face over a call's argument vector: true when the
/// call may format (no arguments, or accepted locales / options).
///
/// # Safety
/// `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn locale_argv_served(argv: *const u64, argc: i64, kind: i64) -> bool {
    if argc <= 0 {
        return true;
    }
    unsafe {
        let (locales, options) = locale_argv(argv, argc);
        __torajs_locale_args_check(locales, options, kind);
        __torajs_throw_check() == 0
    }
}

/// [`locale_argv_served`] for a Date receiver (a NaN date answers
/// "Invalid Date" without consulting either argument).
///
/// # Safety
/// `d` is a live Date cell; `argv` holds `argc` live AnyValues.
unsafe fn date_locale_argv_served(
    d: *const c_void,
    argv: *const u64,
    argc: i64,
    kind: i64,
) -> bool {
    if argc <= 0 {
        return true;
    }
    unsafe {
        let (locales, options) = locale_argv(argv, argc);
        __torajs_locale_args_check_date(d, locales, options, kind);
        __torajs_throw_check() == 0
    }
}

/// A Date `toLocale*String` call on the any lane: `None` for any other
/// method id; undefined, with the throw pending, when the arguments
/// are refused; else the en-US string.
///
/// # Safety
/// `d` is a live Date cell; `argv` holds `argc` live AnyValues.
pub(crate) unsafe fn date_to_locale(
    d: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let (kind, format): (i64, unsafe extern "C" fn(*const c_void) -> *mut u8) = match mid {
        m if m == ANY_METHOD_TO_LOCALE_STRING => {
            (LOCALE_FORMAT_DATE_ANY, __torajs_date_to_locale_string)
        }
        m if m == ANY_METHOD_TO_LOCALE_DATE_STRING => {
            (LOCALE_FORMAT_DATE_DATE, __torajs_date_to_locale_date_string)
        }
        m if m == ANY_METHOD_TO_LOCALE_TIME_STRING => {
            (LOCALE_FORMAT_DATE_TIME, __torajs_date_to_locale_time_string)
        }
        _ => return None,
    };
    unsafe {
        if !date_locale_argv_served(d, argv, argc, kind) {
            return Some(VALUE_UNDEFINED);
        }
        let s = format(d);
        Some(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            4, s as i64,
        ))
    }
}

/// The first two arguments, `options` undefined when absent.
///
/// # Safety
/// `argv` holds `argc >= 1` live AnyValues.
unsafe fn locale_argv(argv: *const u64, argc: i64) -> (u64, u64) {
    unsafe {
        let options = if argc >= 2 {
            *argv.add(1)
        } else {
            VALUE_UNDEFINED
        };
        (*argv, options)
    }
}

/// CanonicalizeLocaleList, down to the validity of every tag: which
/// tag is requested does not change the en-US answer.
unsafe fn canonical_locales(locales: AnyValue) -> Read<()> {
    unsafe {
        if is_undefined(locales) {
            return Ok(());
        }
        if is_null(locales) {
            return Err(Refusal::Type(c"Cannot convert undefined or null to object"));
        }
        let first = if is_string(locales) {
            let tag = __torajs_anyv_to_str(locales) as *mut u8;
            if __torajs_str_locale_check(tag) == 0 {
                __torajs_str_drop(tag as *mut c_void);
                return Err(Refusal::Thrown);
            }
            tag
        } else if is_object(locales) {
            first_valid_tag(as_void_ptr(locales)).map_err(|()| Refusal::Thrown)?
        } else {
            // a number / boolean / symbol / bigint wrapper has no
            // elements: the empty list
            return Ok(());
        };
        if !first.is_null() {
            __torajs_str_drop(first as *mut c_void);
        }
        Ok(())
    }
}
