//! Per-field Date getters — port of `runtime_date.c` L418-525.
//!
//! 16 single-line trampolines split by axis:
//! - UTC: `get_utc_full_year / month / date / hours / minutes /
//!   seconds / milliseconds / day` — branch-free arithmetic via
//!   [`crate::civil::civil_from_days`].
//! - LOCAL: same 8 fns, names without `utc_`, via the in-house
//!   TZif decompose so the result honors the host zone (matches
//!   bun + every other JS engine).
//!
//! RFC 20260713-date-invalid-time: every getter returns f64 (spec
//! number semantics) — NaN when the receiver is null or invalid
//! ([[DateValue]] = NaN, §21.4.4 "if t is NaN, return NaN").

use core::ffi::c_void;

use crate::api::valid_ms;
use crate::civil::civil_from_days;
use crate::tm::localtime_decompose;

/// Decompose `ms` (UNIX ms) into a pure-UTC `(y, m, d, h, min,
/// sec, milli)` 7-tuple. Used by every UTC getter.
pub fn decompose(ms: i64) -> (i32, u32, u32, i32, i32, i32, i32) {
    let day_ms = 86_400_000i64;
    let days = ms.div_euclid(day_ms);
    let mut tod = ms - days * day_ms;
    if tod < 0 {
        tod += day_ms;
    }
    let (y, m, d) = civil_from_days(days);
    let hour = (tod / 3_600_000) as i32;
    let mut rem = tod - hour as i64 * 3_600_000;
    let minute = (rem / 60_000) as i32;
    rem -= minute as i64 * 60_000;
    let second = (rem / 1000) as i32;
    let milli = (rem - second as i64 * 1000) as i32;
    (y, m, d, hour, minute, second, milli)
}

// ---- UTC getters ----

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_full_year(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).0 as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_month(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => (decompose(ms).1 - 1) as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_date(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).2 as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_hours(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).3 as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_minutes(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).4 as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_seconds(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).5 as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_milliseconds(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => decompose(ms).6 as f64,
        None => f64::NAN,
    }
}

/// `.getUTCDay()` — Sun=0..Sat=6 from days-since-epoch + Thu=4 offset.
///
/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_day(d_ptr: *const c_void) -> f64 {
    let Some(ms) = valid_ms(d_ptr) else {
        return f64::NAN;
    };
    let day_ms = 86_400_000i64;
    let mut days = ms.div_euclid(day_ms);
    let tod = ms - days * day_ms;
    if tod < 0 {
        days -= 1;
    }
    (days + 4).rem_euclid(7) as f64
}

// ---- LOCAL-time getters (in-house TZif decompose) ----

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_full_year(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => (localtime_decompose(ms).tm_year + 1900) as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_month(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_mon as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_date(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_mday as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_hours(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_hour as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_minutes(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_min as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_seconds(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_sec as f64,
        None => f64::NAN,
    }
}

/// Sub-second milli — timezone-invariant; bypass the zone pass.
///
/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_milliseconds(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => ms.rem_euclid(1000) as f64,
        None => f64::NAN,
    }
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_day(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_wday as f64,
        None => f64::NAN,
    }
}

/// `d.getTimezoneOffset()` per ES §21.4.4.11 — returns minutes BEHIND
/// UTC for the receiver's instant (NaN when invalid). JS convention
/// flips libc's sign: local-ahead-of-UTC (Asia/Tokyo +09:00) reports
/// `-540`, not `+540`. `local_utoff(secs)` returns the same DST-aware
/// seconds-ahead-of-UTC the local-time getters use, so `-(off / 60)`
/// is the JS answer.
///
/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_timezone_offset(d_ptr: *const c_void) -> f64 {
    let Some(ms) = valid_ms(d_ptr) else {
        return f64::NAN;
    };
    let utc_secs = ms.div_euclid(1000);
    -((crate::tz::local_utoff(utc_secs) as i64 / 60) as f64)
}
