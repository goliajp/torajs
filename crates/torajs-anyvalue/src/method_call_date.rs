//! `Tag::Date` arm of `__torajs_any_method_call`
//! (Any-method-call RFC 20260704 C4-3a) — `d.getTime()` /
//! `d.toISOString()` / `d.setFullYear(y)` where the Date crossed
//! into the `any` world.
//!
//! Mirrors the typed tier's method table
//! (`ssa_lower_call_date_methods.rs`) one id per name onto the
//! `__torajs_date_*` kernels:
//!
//! - **i64 getters** (getTime / valueOf alias / getFullYear / UTC
//!   variants / getDay / getTimezoneOffset / annexB getYear) —
//!   receiver-only call, boxed i64 result.
//! - **string renderings** (toISOString / toJSON alias /
//!   toUTCString / toGMTString alias / toDateString / toString /
//!   toLocale*) —
//!   the kernel returns a fresh +1 Str; the box transfers that
//!   ownership out.
//! - **setters** — arguments decode through `anyv_to_number` (ms
//!   values stay exact: f64 holds integers to 2^53); per-field
//!   setters (setFullYear 3 / setMonth 2 / setDate 1 / setHours 4 /
//!   setMinutes 3 / setSeconds 2 / setMilliseconds 1) pad missing
//!   trailing args with the `DATE_FIELD_KEEP` sentinel exactly like
//!   the typed lowering; the boxed i64 return is the new time value
//!   (ES §21.4.4.20-27).
//!
//! Documented boundary (same class as the C3a width note): tr has
//! no Invalid-Date representation, so a missing/NaN `setTime` /
//! `setYear` argument (which the typed tier's checker rejects
//! outright) does not produce bun's `Invalid Date` — the decode
//! collapses to 0 / KEEP. Trailing extras beyond a method's arity
//! are ignored (they were already evaluated into argv by the
//! lowerer, so ES side-effect order holds).

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_GET_DATE, ANY_METHOD_GET_DAY, ANY_METHOD_GET_FULL_YEAR, ANY_METHOD_GET_HOURS,
    ANY_METHOD_GET_MILLISECONDS, ANY_METHOD_GET_MINUTES, ANY_METHOD_GET_MONTH,
    ANY_METHOD_GET_SECONDS, ANY_METHOD_GET_TIME, ANY_METHOD_GET_TIMEZONE_OFFSET,
    ANY_METHOD_GET_UTC_DATE, ANY_METHOD_GET_UTC_DAY, ANY_METHOD_GET_UTC_FULL_YEAR,
    ANY_METHOD_GET_UTC_HOURS, ANY_METHOD_GET_UTC_MILLISECONDS, ANY_METHOD_GET_UTC_MINUTES,
    ANY_METHOD_GET_UTC_MONTH, ANY_METHOD_GET_UTC_SECONDS, ANY_METHOD_GET_YEAR, ANY_METHOD_SET_DATE,
    ANY_METHOD_SET_FULL_YEAR, ANY_METHOD_SET_HOURS, ANY_METHOD_SET_MILLISECONDS,
    ANY_METHOD_SET_MINUTES, ANY_METHOD_SET_MONTH, ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME,
    ANY_METHOD_SET_YEAR, ANY_METHOD_TO_DATE_STRING, ANY_METHOD_TO_GMT_STRING,
    ANY_METHOD_TO_ISO_STRING, ANY_METHOD_TO_JSON, ANY_METHOD_TO_LOCALE_DATE_STRING,
    ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_LOCALE_TIME_STRING, ANY_METHOD_TO_STRING,
    ANY_METHOD_TO_UTC_STRING, ANY_METHOD_VALUE_OF,
};

use crate::method_call::method_not_a_function;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::{__torajs_anyv_box_from_pair, __torajs_anyv_box_i64};
use crate::nanbox_ffi::__torajs_anyv_to_number;

/// Missing-trailing-field sentinel — mirror of torajs-date
/// `api.rs::DATE_FIELD_KEEP` (per-field setters keep the current
/// component).
const DATE_FIELD_KEEP: i64 = i64::MIN;

unsafe extern "C" {
    fn __torajs_date_get_time(d: *const c_void) -> i64;
    fn __torajs_date_get_year(d: *const c_void) -> i64;
    fn __torajs_date_get_full_year(d: *const c_void) -> i64;
    fn __torajs_date_get_month(d: *const c_void) -> i64;
    fn __torajs_date_get_date(d: *const c_void) -> i64;
    fn __torajs_date_get_hours(d: *const c_void) -> i64;
    fn __torajs_date_get_minutes(d: *const c_void) -> i64;
    fn __torajs_date_get_seconds(d: *const c_void) -> i64;
    fn __torajs_date_get_milliseconds(d: *const c_void) -> i64;
    fn __torajs_date_get_day(d: *const c_void) -> i64;
    fn __torajs_date_get_timezone_offset(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_full_year(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_month(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_date(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_hours(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_minutes(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_seconds(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_milliseconds(d: *const c_void) -> i64;
    fn __torajs_date_get_utc_day(d: *const c_void) -> i64;
    fn __torajs_date_to_iso_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_gmt_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_date_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_date_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_to_locale_time_string(d: *const c_void) -> *mut u8;
    fn __torajs_date_set_time(d: *mut c_void, ms: i64) -> i64;
    fn __torajs_date_set_year(d: *mut c_void, y: i64) -> i64;
    fn __torajs_date_set_full_year(d: *mut c_void, y: i64, mo: i64, dd: i64) -> i64;
    fn __torajs_date_set_month(d: *mut c_void, mo: i64, dd: i64) -> i64;
    fn __torajs_date_set_date(d: *mut c_void, dd: i64) -> i64;
    fn __torajs_date_set_hours(d: *mut c_void, h: i64, mi: i64, s: i64, ms: i64) -> i64;
    fn __torajs_date_set_minutes(d: *mut c_void, mi: i64, s: i64, ms: i64) -> i64;
    fn __torajs_date_set_seconds(d: *mut c_void, s: i64, ms: i64) -> i64;
    fn __torajs_date_set_milliseconds(d: *mut c_void, ms: i64) -> i64;
}

/// `Tag::Date` arm — id-switch onto the torajs-date kernels (see
/// module doc).
pub(crate) unsafe fn date_method(
    d: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // Setter-argument decode: in-range args go through ToNumber,
    // missing trailing fields ride the KEEP sentinel (typed-lowering
    // parity).
    let field = |i: i64| -> i64 {
        if i < argc {
            let av = unsafe { *argv.add(i as usize) };
            if av == VALUE_UNDEFINED {
                DATE_FIELD_KEEP
            } else {
                unsafe { __torajs_anyv_to_number(av) as i64 }
            }
        } else {
            DATE_FIELD_KEEP
        }
    };
    unsafe {
        let i = |n: i64| __torajs_anyv_box_i64(n);
        let s = |p: *mut u8| __torajs_anyv_box_from_pair(4, p as i64);
        match mid {
            m if m == ANY_METHOD_GET_TIME || m == ANY_METHOD_VALUE_OF => {
                i(__torajs_date_get_time(d))
            }
            m if m == ANY_METHOD_TO_ISO_STRING || m == ANY_METHOD_TO_JSON => {
                s(__torajs_date_to_iso_string(d))
            }
            m if m == ANY_METHOD_GET_FULL_YEAR => i(__torajs_date_get_full_year(d)),
            m if m == ANY_METHOD_GET_UTC_FULL_YEAR => i(__torajs_date_get_utc_full_year(d)),
            m if m == ANY_METHOD_GET_MONTH => i(__torajs_date_get_month(d)),
            m if m == ANY_METHOD_GET_UTC_MONTH => i(__torajs_date_get_utc_month(d)),
            m if m == ANY_METHOD_GET_DATE => i(__torajs_date_get_date(d)),
            m if m == ANY_METHOD_GET_UTC_DATE => i(__torajs_date_get_utc_date(d)),
            m if m == ANY_METHOD_GET_HOURS => i(__torajs_date_get_hours(d)),
            m if m == ANY_METHOD_GET_UTC_HOURS => i(__torajs_date_get_utc_hours(d)),
            m if m == ANY_METHOD_GET_MINUTES => i(__torajs_date_get_minutes(d)),
            m if m == ANY_METHOD_GET_UTC_MINUTES => i(__torajs_date_get_utc_minutes(d)),
            m if m == ANY_METHOD_GET_SECONDS => i(__torajs_date_get_seconds(d)),
            m if m == ANY_METHOD_GET_UTC_SECONDS => i(__torajs_date_get_utc_seconds(d)),
            m if m == ANY_METHOD_GET_MILLISECONDS => i(__torajs_date_get_milliseconds(d)),
            m if m == ANY_METHOD_GET_UTC_MILLISECONDS => i(__torajs_date_get_utc_milliseconds(d)),
            m if m == ANY_METHOD_GET_DAY => i(__torajs_date_get_day(d)),
            m if m == ANY_METHOD_GET_UTC_DAY => i(__torajs_date_get_utc_day(d)),
            m if m == ANY_METHOD_GET_TIMEZONE_OFFSET => i(__torajs_date_get_timezone_offset(d)),
            m if m == ANY_METHOD_GET_YEAR => i(__torajs_date_get_year(d)),
            m if m == ANY_METHOD_TO_GMT_STRING || m == ANY_METHOD_TO_UTC_STRING => {
                s(__torajs_date_to_gmt_string(d))
            }
            m if m == ANY_METHOD_TO_DATE_STRING => s(__torajs_date_to_date_string(d)),
            m if m == ANY_METHOD_TO_STRING => s(__torajs_date_to_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_STRING => s(__torajs_date_to_locale_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_DATE_STRING => s(__torajs_date_to_locale_date_string(d)),
            m if m == ANY_METHOD_TO_LOCALE_TIME_STRING => s(__torajs_date_to_locale_time_string(d)),
            m if m == ANY_METHOD_SET_TIME => i(__torajs_date_set_time(d, field(0))),
            m if m == ANY_METHOD_SET_YEAR => i(__torajs_date_set_year(d, field(0))),
            m if m == ANY_METHOD_SET_FULL_YEAR => {
                i(__torajs_date_set_full_year(d, field(0), field(1), field(2)))
            }
            m if m == ANY_METHOD_SET_MONTH => i(__torajs_date_set_month(d, field(0), field(1))),
            m if m == ANY_METHOD_SET_DATE => i(__torajs_date_set_date(d, field(0))),
            m if m == ANY_METHOD_SET_HOURS => i(__torajs_date_set_hours(
                d,
                field(0),
                field(1),
                field(2),
                field(3),
            )),
            m if m == ANY_METHOD_SET_MINUTES => {
                i(__torajs_date_set_minutes(d, field(0), field(1), field(2)))
            }
            m if m == ANY_METHOD_SET_SECONDS => i(__torajs_date_set_seconds(d, field(0), field(1))),
            m if m == ANY_METHOD_SET_MILLISECONDS => i(__torajs_date_set_milliseconds(d, field(0))),
            _ => method_not_a_function(),
        }
    }
}
